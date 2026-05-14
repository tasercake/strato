//! Deterministic callable-level call graph data model.

use std::collections::BTreeMap;

use crate::types::SourceLocation;

/// Stable graph node identifier assigned in deterministic node order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId(pub usize);

/// Callable categories represented by the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallableKind {
    /// Top-level or nested synchronous function.
    Function,
    /// Top-level or nested asynchronous function.
    AsyncFunction,
    /// Instance method.
    Method,
    /// Asynchronous instance method.
    AsyncMethod,
    /// Property getter.
    Property,
    /// Class method.
    ClassMethod,
    /// Static method.
    StaticMethod,
    /// Lambda callable.
    Lambda,
    /// Python dunder method.
    DunderMethod,
}

/// Blocking state owned by graph/annotation/propagation phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockingStatus {
    /// No blocking information is known yet.
    Unknown,
    /// Explicitly known blocking root.
    KnownBlocking,
    /// Explicitly known non-blocking root.
    KnownNonBlocking,
    /// Blocking inferred by later propagation.
    PropagatedBlocking,
}

/// One callable graph node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallGraphNode {
    /// Stable node id.
    pub id: NodeId,
    /// Module-scoped stable identity used for graph lookup.
    pub identity: String,
    /// Deterministic callable name for display.
    pub qualified_name: String,
    /// Callable kind.
    pub kind: CallableKind,
    /// Whether the callable is asynchronous.
    pub is_async: bool,
    /// Source location, absent for external phantom nodes.
    pub location: Option<SourceLocation>,
    /// Current blocking status.
    pub blocking_status: BlockingStatus,
}

/// Edge mechanism between two callable nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    /// Direct function call.
    DirectCall,
    /// Method invocation.
    MethodCall,
    /// Property getter access.
    PropertyAccess,
    /// Implicit dunder operation.
    ImplicitDunder,
    /// Super method call.
    SuperCall,
    /// Decorator application.
    DecoratorCall,
}

/// One directed call edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallEdge {
    /// Caller node id.
    pub from: NodeId,
    /// Callee node id.
    pub to: NodeId,
    /// Edge mechanism.
    pub kind: EdgeKind,
    /// Source location for the call/access.
    pub location: SourceLocation,
    /// True when the edge is protected by an executor wrapper.
    pub in_executor: bool,
    /// Wrapper node or phantom node used for attribution.
    pub via: Option<NodeId>,
    /// True for synthetic executor-protected callable-argument edges.
    pub protected: bool,
}

/// Deterministic call graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallGraph {
    nodes: Vec<CallGraphNode>,
    node_by_name: BTreeMap<String, NodeId>,
    edges: Vec<CallEdge>,
}

impl CallGraph {
    /// Returns all nodes in stable id order.
    #[must_use]
    pub fn nodes(&self) -> &[CallGraphNode] {
        &self.nodes
    }

    /// Returns all edges in deterministic order.
    #[must_use]
    pub fn edges(&self) -> &[CallEdge] {
        &self.edges
    }

    /// Returns a node by lookup key.
    #[must_use]
    pub fn node(&self, key: &str) -> Option<&CallGraphNode> {
        self.node_by_name
            .get(key)
            .and_then(|id| self.nodes.get(id.0))
    }

    /// Returns a node id by lookup key.
    #[must_use]
    pub fn node_id(&self, key: &str) -> Option<NodeId> {
        self.node_by_name.get(key).copied()
    }

    pub(crate) fn add_node(
        &mut self,
        qualified_name: String,
        kind: CallableKind,
        is_async: bool,
        location: Option<SourceLocation>,
        blocking_status: BlockingStatus,
    ) -> NodeId {
        self.add_node_with_identity(
            qualified_name.clone(),
            qualified_name,
            kind,
            is_async,
            location,
            blocking_status,
        )
    }

    pub(crate) fn add_node_with_identity(
        &mut self,
        identity: String,
        qualified_name: String,
        kind: CallableKind,
        is_async: bool,
        location: Option<SourceLocation>,
        blocking_status: BlockingStatus,
    ) -> NodeId {
        if let Some(id) = self.node_by_name.get(&identity) {
            return *id;
        }
        let id = NodeId(self.nodes.len());
        self.nodes.push(CallGraphNode {
            id,
            identity: identity.clone(),
            qualified_name: qualified_name.clone(),
            kind,
            is_async,
            location,
            blocking_status,
        });
        self.node_by_name.insert(identity, id);
        self.node_by_name.entry(qualified_name).or_insert(id);
        id
    }

    pub(crate) fn add_edge(&mut self, edge: CallEdge) {
        if !self.edges.contains(&edge) {
            self.edges.push(edge);
            self.edges.sort();
        }
    }

    pub(crate) fn set_blocking_status(&mut self, id: NodeId, status: BlockingStatus) {
        if let Some(node) = self.nodes.get_mut(id.0) {
            node.blocking_status = status;
        }
    }

    /// Deterministic node snapshot useful for tests and evidence.
    #[must_use]
    pub fn node_snapshots(&self) -> Vec<String> {
        self.nodes
            .iter()
            .map(|node| {
                format!(
                    "{} [{:?} async={} status={:?} location={}]",
                    node.qualified_name,
                    node.kind,
                    node.is_async,
                    node.blocking_status,
                    node.location.map_or_else(
                        || "-".to_string(),
                        |location| format!("{}..{}", location.start, location.end)
                    )
                )
            })
            .collect()
    }

    /// Deterministic edge snapshot useful for tests and evidence.
    #[must_use]
    pub fn edge_snapshots(&self) -> Vec<String> {
        self.edges
            .iter()
            .map(|edge| {
                let from = &self.nodes[edge.from.0].qualified_name;
                let to = &self.nodes[edge.to.0].qualified_name;
                let via = edge.via.map_or_else(
                    || "-".to_string(),
                    |id| self.nodes[id.0].qualified_name.clone(),
                );
                format!(
                    "{from} -> {to} [{:?} executor={} via={} protected={}]",
                    edge.kind, edge.in_executor, via, edge.protected
                )
            })
            .collect()
    }
}
