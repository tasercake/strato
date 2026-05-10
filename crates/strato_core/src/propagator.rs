//! Phase 6 SCC-based blocking propagation.

use std::{cmp::Ordering, collections::BTreeMap, collections::BTreeSet};

use crate::{
    graph::{BlockingStatus, CallEdge, CallGraph, EdgeKind, NodeId},
    types::SourceLocation,
};

/// Propagation output consumed by later reporting phases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropagationResult {
    /// Strongly connected components in deterministic component order.
    pub sccs: Vec<Vec<NodeId>>,
    /// Deterministic condensation DAG edges.
    pub condensation_edges: Vec<CondensedEdge>,
    /// Shortest blocking reason for each known or propagated blocking node.
    pub blocking_reasons: BTreeMap<NodeId, BlockingReason>,
}

/// One edge in the SCC condensation DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CondensedEdge {
    /// Caller SCC id.
    pub from_scc: usize,
    /// Callee SCC id.
    pub to_scc: usize,
    /// True only when every underlying call edge is executor-protected.
    pub all_calls_in_executor: bool,
}

/// The deterministic root and call path that make a node blocking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingReason {
    /// Ultimate blocking node.
    pub root_cause: NodeId,
    /// Caller-to-callee chain from the propagated node to the root.
    pub chain_links: Vec<ChainLink>,
}

/// One call in a blocking path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChainLink {
    /// Calling function qualified name.
    pub function_name: String,
    /// Calling function definition location.
    pub function_location: Option<SourceLocation>,
    /// Call expression location.
    pub call_site_location: Option<SourceLocation>,
    /// Callee qualified name.
    pub callee_name: String,
    /// Semantic edge kind.
    pub edge_kind: EdgeKind,
    /// Whether the caller is async.
    pub is_async: bool,
    /// Whether the caller is first-party.
    pub is_first_party: bool,
}

#[derive(Debug, Clone)]
struct Condensation {
    edges: Vec<CondensedEdge>,
    outgoing_edges: BTreeMap<usize, Vec<CondensedEdge>>,
    edges_by_scc_pair: BTreeMap<(usize, usize), Vec<CallEdge>>,
    scc_by_node: Vec<usize>,
}

/// Propagate blocking facts over a Tarjan condensation DAG.
#[must_use]
pub fn propagate_blocking(graph: &mut CallGraph) -> PropagationResult {
    let sccs = tarjan_scc(graph);
    let condensation = build_condensation(graph, &sccs);
    let topo_order = topological_order(sccs.len(), &condensation.edges);
    let incoming_unprotected = incoming_unprotected_edges(graph);
    let mut paths = vec![None; graph.nodes().len()];

    for scc_id in topo_order.into_iter().rev() {
        process_scc(
            graph,
            &sccs[scc_id],
            scc_id,
            &condensation,
            &incoming_unprotected,
            &mut paths,
        );
    }

    let blocking_reasons = graph
        .nodes()
        .iter()
        .filter(|node| {
            matches!(
                node.blocking_status,
                BlockingStatus::KnownBlocking | BlockingStatus::PropagatedBlocking
            )
        })
        .filter_map(|node| paths[node.id.0].clone().map(|reason| (node.id, reason)))
        .collect();

    PropagationResult {
        sccs,
        condensation_edges: condensation.edges,
        blocking_reasons,
    }
}

/// Decompose the call graph into SCCs using Tarjan's algorithm.
#[must_use]
pub fn tarjan_scc(graph: &CallGraph) -> Vec<Vec<NodeId>> {
    let adjacency = adjacency_by_node(graph);
    let mut state = TarjanState::new(adjacency);
    for node in graph.nodes() {
        if state.indexes[node.id.0].is_none() {
            state.connect(node.id);
        }
    }
    state
        .components
        .sort_by(|left, right| left.first().cmp(&right.first()));
    state.components
}

fn process_scc(
    graph: &mut CallGraph,
    scc: &[NodeId],
    scc_id: usize,
    condensation: &Condensation,
    incoming_unprotected: &[Vec<CallEdge>],
    paths: &mut [Option<BlockingReason>],
) {
    let mut frontier = BTreeSet::new();

    for node_id in scc {
        if graph.nodes()[node_id.0].blocking_status == BlockingStatus::KnownBlocking {
            let reason = BlockingReason {
                root_cause: *node_id,
                chain_links: Vec::new(),
            };
            if relax_path(paths, *node_id, reason, graph) {
                frontier.insert(*node_id);
            }
        }
    }

    if let Some(outgoing_edges) = condensation.outgoing_edges.get(&scc_id) {
        for condensed_edge in outgoing_edges {
            if condensed_edge.all_calls_in_executor {
                continue;
            }
            if let Some(edges) = condensation
                .edges_by_scc_pair
                .get(&(condensed_edge.from_scc, condensed_edge.to_scc))
            {
                for edge in edges.iter().filter(|edge| !edge_is_protected(edge)) {
                    let Some(tail_reason) = paths[edge.to.0].clone() else {
                        continue;
                    };
                    let reason = prepend_edge(graph, edge, &tail_reason);
                    if relax_path(paths, edge.from, reason, graph) {
                        frontier.insert(edge.from);
                    }
                }
            }
        }
    }

    while let Some(node_id) = frontier.pop_first() {
        let Some(tail_reason) = paths[node_id.0].clone() else {
            continue;
        };
        for edge in &incoming_unprotected[node_id.0] {
            if condensation.scc_by_node[edge.from.0] != scc_id {
                continue;
            }
            let reason = prepend_edge(graph, edge, &tail_reason);
            if relax_path(paths, edge.from, reason, graph) {
                frontier.insert(edge.from);
            }
        }
    }

    for node_id in scc {
        if graph.nodes()[node_id.0].blocking_status == BlockingStatus::Unknown
            && paths[node_id.0].is_some()
        {
            graph.set_blocking_status(*node_id, BlockingStatus::PropagatedBlocking);
        }
    }
}

fn relax_path(
    paths: &mut [Option<BlockingReason>],
    node_id: NodeId,
    candidate: BlockingReason,
    graph: &CallGraph,
) -> bool {
    if graph.nodes()[node_id.0].blocking_status == BlockingStatus::KnownNonBlocking {
        return false;
    }
    let should_replace = paths[node_id.0]
        .as_ref()
        .is_none_or(|existing| compare_reasons(&candidate, existing, graph).is_lt());
    if should_replace {
        paths[node_id.0] = Some(candidate);
    }
    should_replace
}

fn compare_reasons(left: &BlockingReason, right: &BlockingReason, graph: &CallGraph) -> Ordering {
    left.chain_links
        .len()
        .cmp(&right.chain_links.len())
        .then_with(|| {
            graph.nodes()[left.root_cause.0]
                .qualified_name
                .cmp(&graph.nodes()[right.root_cause.0].qualified_name)
        })
        .then_with(|| left.chain_links.cmp(&right.chain_links))
        .then_with(|| left.root_cause.cmp(&right.root_cause))
}

fn prepend_edge(
    graph: &CallGraph,
    edge: &CallEdge,
    tail_reason: &BlockingReason,
) -> BlockingReason {
    let source_node = &graph.nodes()[edge.from.0];
    let target_node = &graph.nodes()[edge.to.0];
    let mut chain_links = Vec::with_capacity(tail_reason.chain_links.len() + 1);
    chain_links.push(ChainLink {
        function_name: source_node.identity.clone(),
        function_location: source_node.location,
        call_site_location: Some(edge.location),
        callee_name: target_node.identity.clone(),
        edge_kind: edge.kind,
        is_async: source_node.is_async,
        is_first_party: source_node.location.is_some(),
    });
    chain_links.extend(tail_reason.chain_links.iter().cloned());
    BlockingReason {
        root_cause: tail_reason.root_cause,
        chain_links,
    }
}

fn build_condensation(graph: &CallGraph, sccs: &[Vec<NodeId>]) -> Condensation {
    let mut scc_by_node = vec![0; graph.nodes().len()];
    for (scc_id, scc) in sccs.iter().enumerate() {
        for node_id in scc {
            scc_by_node[node_id.0] = scc_id;
        }
    }

    let mut protection_by_pair = BTreeMap::<(usize, usize), bool>::new();
    let mut edges_by_scc_pair = BTreeMap::<(usize, usize), Vec<CallEdge>>::new();
    for edge in graph.edges() {
        let from_scc = scc_by_node[edge.from.0];
        let to_scc = scc_by_node[edge.to.0];
        if from_scc == to_scc {
            continue;
        }
        let pair = (from_scc, to_scc);
        protection_by_pair
            .entry(pair)
            .and_modify(|all_protected| *all_protected &= edge_is_protected(edge))
            .or_insert_with(|| edge_is_protected(edge));
        edges_by_scc_pair
            .entry(pair)
            .or_default()
            .push(edge.clone());
    }

    let edges = protection_by_pair
        .into_iter()
        .map(
            |((from_scc, to_scc), all_calls_in_executor)| CondensedEdge {
                from_scc,
                to_scc,
                all_calls_in_executor,
            },
        )
        .collect::<Vec<_>>();
    let mut outgoing_edges = BTreeMap::<usize, Vec<CondensedEdge>>::new();
    for edge in &edges {
        outgoing_edges.entry(edge.from_scc).or_default().push(*edge);
    }

    Condensation {
        edges,
        outgoing_edges,
        edges_by_scc_pair,
        scc_by_node,
    }
}

fn topological_order(scc_count: usize, edges: &[CondensedEdge]) -> Vec<usize> {
    let mut adjacency = vec![BTreeSet::new(); scc_count];
    let mut indegrees = vec![0_usize; scc_count];
    for edge in edges {
        if adjacency[edge.from_scc].insert(edge.to_scc) {
            indegrees[edge.to_scc] += 1;
        }
    }

    let mut ready = indegrees
        .iter()
        .enumerate()
        .filter_map(|(scc_id, indegree)| (*indegree == 0).then_some(scc_id))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(scc_count);

    while let Some(scc_id) = ready.pop_first() {
        order.push(scc_id);
        for target in adjacency[scc_id].clone() {
            indegrees[target] -= 1;
            if indegrees[target] == 0 {
                ready.insert(target);
            }
        }
    }

    if order.len() != scc_count {
        let ordered = order.iter().copied().collect::<BTreeSet<_>>();
        order.extend((0..scc_count).filter(|scc_id| !ordered.contains(scc_id)));
    }

    order
}

fn adjacency_by_node(graph: &CallGraph) -> Vec<Vec<NodeId>> {
    let mut adjacency = vec![BTreeSet::new(); graph.nodes().len()];
    for edge in graph.edges() {
        adjacency[edge.from.0].insert(edge.to);
    }
    adjacency
        .into_iter()
        .map(|targets| targets.into_iter().collect())
        .collect()
}

fn incoming_unprotected_edges(graph: &CallGraph) -> Vec<Vec<CallEdge>> {
    let mut incoming = vec![Vec::new(); graph.nodes().len()];
    for edge in graph.edges().iter().filter(|edge| !edge_is_protected(edge)) {
        incoming[edge.to.0].push(edge.clone());
    }
    incoming
}

fn edge_is_protected(edge: &CallEdge) -> bool {
    edge.protected || edge.in_executor
}

#[derive(Debug, Clone)]
struct TarjanState {
    next_index: usize,
    stack: Vec<NodeId>,
    on_stack: Vec<bool>,
    indexes: Vec<Option<usize>>,
    lowlinks: Vec<usize>,
    adjacency: Vec<Vec<NodeId>>,
    components: Vec<Vec<NodeId>>,
}

impl TarjanState {
    fn new(adjacency: Vec<Vec<NodeId>>) -> Self {
        let node_count = adjacency.len();
        Self {
            next_index: 0,
            stack: Vec::new(),
            on_stack: vec![false; node_count],
            indexes: vec![None; node_count],
            lowlinks: vec![0; node_count],
            adjacency,
            components: Vec::new(),
        }
    }

    fn connect(&mut self, node_id: NodeId) {
        self.indexes[node_id.0] = Some(self.next_index);
        self.lowlinks[node_id.0] = self.next_index;
        self.next_index += 1;
        self.stack.push(node_id);
        self.on_stack[node_id.0] = true;

        for target in self.adjacency[node_id.0].clone() {
            if self.indexes[target.0].is_none() {
                self.connect(target);
                self.lowlinks[node_id.0] = self.lowlinks[node_id.0].min(self.lowlinks[target.0]);
            } else if self.on_stack[target.0] {
                let target_index = self.indexes[target.0].expect("stacked node has an index");
                self.lowlinks[node_id.0] = self.lowlinks[node_id.0].min(target_index);
            }
        }

        if self.lowlinks[node_id.0] == self.indexes[node_id.0].expect("node has an index") {
            let mut component = Vec::new();
            while let Some(member) = self.stack.pop() {
                self.on_stack[member.0] = false;
                component.push(member);
                if member == node_id {
                    break;
                }
            }
            component.sort();
            self.components.push(component);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        graph::{BlockingStatus, CallEdge, CallGraph, CallableKind, EdgeKind, NodeId},
        propagator::{propagate_blocking, tarjan_scc},
        types::SourceLocation,
    };

    fn loc(start: u32) -> SourceLocation {
        SourceLocation {
            start,
            end: start + 1,
        }
    }

    fn function(
        graph: &mut CallGraph,
        name: &str,
        is_async: bool,
        status: BlockingStatus,
        start: u32,
    ) -> NodeId {
        graph.add_node(
            name.to_string(),
            if is_async {
                CallableKind::AsyncFunction
            } else {
                CallableKind::Function
            },
            is_async,
            Some(loc(start)),
            status,
        )
    }

    fn external(graph: &mut CallGraph, name: &str, status: BlockingStatus) -> NodeId {
        graph.add_node(
            name.to_string(),
            CallableKind::Function,
            false,
            None,
            status,
        )
    }

    fn call(graph: &mut CallGraph, from: NodeId, to: NodeId, protected: bool, start: u32) {
        graph.add_edge(CallEdge {
            from,
            to,
            kind: EdgeKind::DirectCall,
            location: loc(start),
            in_executor: protected,
            via: None,
            protected,
        });
    }

    fn statuses(graph: &CallGraph) -> BTreeMap<String, BlockingStatus> {
        graph
            .nodes()
            .iter()
            .map(|node| (node.qualified_name.clone(), node.blocking_status))
            .collect()
    }

    fn scc_names(graph: &CallGraph, sccs: &[Vec<NodeId>]) -> Vec<Vec<String>> {
        sccs.iter()
            .map(|scc| {
                scc.iter()
                    .map(|id| graph.nodes()[id.0].qualified_name.clone())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn scc_containing(sccs: &[Vec<NodeId>], node: NodeId) -> usize {
        sccs.iter()
            .position(|scc| scc.contains(&node))
            .expect("node belongs to one SCC")
    }

    fn reason_snapshot(graph: &CallGraph, result: &super::PropagationResult) -> Vec<String> {
        result
            .blocking_reasons
            .iter()
            .map(|(node, reason)| {
                let node_name = &graph.nodes()[node.0].qualified_name;
                let root_name = &graph.nodes()[reason.root_cause.0].qualified_name;
                let chain = reason
                    .chain_links
                    .iter()
                    .map(|link| {
                        format!(
                            "{}@{}->{}",
                            link.function_name,
                            link.call_site_location.map_or_else(
                                || "-".to_string(),
                                |location| location.start.to_string()
                            ),
                            link.callee_name
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("|");
                format!("{node_name}:{root_name}:{chain}")
            })
            .collect()
    }

    #[test]
    fn propagator_tarjan_simple_chain_marks_callers_and_paths() {
        let mut graph = CallGraph::default();
        let handler = function(&mut graph, "handler", true, BlockingStatus::Unknown, 1);
        let helper = function(&mut graph, "helper", false, BlockingStatus::Unknown, 10);
        let sleep = external(&mut graph, "time.sleep", BlockingStatus::KnownBlocking);
        call(&mut graph, handler, helper, false, 20);
        call(&mut graph, helper, sleep, false, 30);

        let result = propagate_blocking(&mut graph);

        let statuses = statuses(&graph);
        assert_eq!(statuses["handler"], BlockingStatus::PropagatedBlocking);
        assert_eq!(statuses["helper"], BlockingStatus::PropagatedBlocking);
        assert_eq!(statuses["time.sleep"], BlockingStatus::KnownBlocking);

        let handler_reason = &result.blocking_reasons[&handler];
        assert_eq!(handler_reason.root_cause, sleep);
        assert_eq!(handler_reason.chain_links.len(), 2);
        assert_eq!(handler_reason.chain_links[0].function_name, "handler");
        assert_eq!(handler_reason.chain_links[0].callee_name, "helper");
        assert_eq!(handler_reason.chain_links[1].function_name, "helper");
        assert_eq!(handler_reason.chain_links[1].callee_name, "time.sleep");
    }

    #[test]
    fn tarjan_scc_cycle_propagates_blocking_to_unannotated_members() {
        let mut graph = CallGraph::default();
        let cycle_a = function(&mut graph, "cycle_a", false, BlockingStatus::Unknown, 1);
        let cycle_b = function(&mut graph, "cycle_b", false, BlockingStatus::Unknown, 10);
        let sleep = external(&mut graph, "time.sleep", BlockingStatus::KnownBlocking);
        call(&mut graph, cycle_a, cycle_b, false, 20);
        call(&mut graph, cycle_b, cycle_a, false, 30);
        call(&mut graph, cycle_b, sleep, false, 40);

        let sccs = tarjan_scc(&graph);
        assert!(
            scc_names(&graph, &sccs).contains(&vec!["cycle_a".to_string(), "cycle_b".to_string(),])
        );

        let _ = propagate_blocking(&mut graph);

        let statuses = statuses(&graph);
        assert_eq!(statuses["cycle_a"], BlockingStatus::PropagatedBlocking);
        assert_eq!(statuses["cycle_b"], BlockingStatus::PropagatedBlocking);
    }

    #[test]
    fn propagator_protected_executor_edge_prevents_propagation_and_unprotected_equivalent_propagates()
     {
        let mut graph = CallGraph::default();
        let protected_handler = function(
            &mut graph,
            "protected_handler",
            true,
            BlockingStatus::Unknown,
            1,
        );
        let unprotected_handler = function(
            &mut graph,
            "unprotected_handler",
            true,
            BlockingStatus::Unknown,
            10,
        );
        let sleep = external(&mut graph, "time.sleep", BlockingStatus::KnownBlocking);
        call(&mut graph, protected_handler, sleep, true, 20);
        call(&mut graph, unprotected_handler, sleep, false, 30);

        let result = propagate_blocking(&mut graph);

        let statuses = statuses(&graph);
        assert_eq!(statuses["protected_handler"], BlockingStatus::Unknown);
        assert_eq!(
            statuses["unprotected_handler"],
            BlockingStatus::PropagatedBlocking
        );
        assert!(!result.blocking_reasons.contains_key(&protected_handler));
        assert!(result.blocking_reasons.contains_key(&unprotected_handler));
    }

    #[test]
    fn propagator_mixed_condensed_edges_are_unprotected_if_any_underlying_edge_is_unprotected() {
        let mut graph = CallGraph::default();
        let caller_a = function(&mut graph, "caller_a", false, BlockingStatus::Unknown, 1);
        let caller_b = function(&mut graph, "caller_b", false, BlockingStatus::Unknown, 10);
        let sleep = external(&mut graph, "time.sleep", BlockingStatus::KnownBlocking);
        call(&mut graph, caller_a, caller_b, false, 20);
        call(&mut graph, caller_b, caller_a, false, 30);
        call(&mut graph, caller_a, sleep, true, 40);
        call(&mut graph, caller_b, sleep, false, 50);

        let result = propagate_blocking(&mut graph);

        let caller_scc = scc_containing(&result.sccs, caller_a);
        let root_scc = scc_containing(&result.sccs, sleep);
        assert!(result.condensation_edges.iter().any(|edge| {
            edge.from_scc == caller_scc && edge.to_scc == root_scc && !edge.all_calls_in_executor
        }));
        let statuses = statuses(&graph);
        assert_eq!(statuses["caller_a"], BlockingStatus::PropagatedBlocking);
        assert_eq!(statuses["caller_b"], BlockingStatus::PropagatedBlocking);
    }

    #[test]
    fn propagator_known_non_blocking_remains_local_only_and_does_not_erase_peer_or_callee() {
        let mut graph = CallGraph::default();
        let safe = function(
            &mut graph,
            "safe",
            false,
            BlockingStatus::KnownNonBlocking,
            1,
        );
        let unsafe_peer = function(
            &mut graph,
            "unsafe_peer",
            false,
            BlockingStatus::Unknown,
            10,
        );
        let sleep = external(&mut graph, "time.sleep", BlockingStatus::KnownBlocking);
        call(&mut graph, safe, unsafe_peer, false, 20);
        call(&mut graph, unsafe_peer, safe, false, 30);
        call(&mut graph, unsafe_peer, sleep, false, 40);

        let result = propagate_blocking(&mut graph);

        let statuses = statuses(&graph);
        assert_eq!(statuses["safe"], BlockingStatus::KnownNonBlocking);
        assert_eq!(statuses["unsafe_peer"], BlockingStatus::PropagatedBlocking);
        assert_eq!(statuses["time.sleep"], BlockingStatus::KnownBlocking);
        assert!(!result.blocking_reasons.contains_key(&safe));
        assert!(result.blocking_reasons.contains_key(&unsafe_peer));
    }

    #[test]
    fn propagator_deterministic_repeated_runs_choose_same_paths() {
        let mut graph = CallGraph::default();
        let handler = function(&mut graph, "handler", true, BlockingStatus::Unknown, 1);
        let helper_a = function(&mut graph, "helper_a", false, BlockingStatus::Unknown, 10);
        let helper_b = function(&mut graph, "helper_b", false, BlockingStatus::Unknown, 20);
        let root_a = external(&mut graph, "a.blocking", BlockingStatus::KnownBlocking);
        let root_b = external(&mut graph, "b.blocking", BlockingStatus::KnownBlocking);
        call(&mut graph, handler, helper_b, false, 50);
        call(&mut graph, handler, helper_a, false, 40);
        call(&mut graph, helper_b, root_b, false, 70);
        call(&mut graph, helper_a, root_a, false, 60);

        let mut first_graph = graph.clone();
        let mut second_graph = graph.clone();
        let first = propagate_blocking(&mut first_graph);
        let second = propagate_blocking(&mut second_graph);

        assert_eq!(first_graph.node_snapshots(), second_graph.node_snapshots());
        assert_eq!(
            reason_snapshot(&first_graph, &first),
            reason_snapshot(&second_graph, &second)
        );
    }

    #[test]
    fn propagator_shortest_path_prefers_shortest_then_lexicographic_root_and_call_site() {
        let mut graph = CallGraph::default();
        let start = function(&mut graph, "start", true, BlockingStatus::Unknown, 1);
        let mid = function(&mut graph, "mid", false, BlockingStatus::Unknown, 10);
        let root_a = external(&mut graph, "a.blocking", BlockingStatus::KnownBlocking);
        let root_b = external(&mut graph, "b.blocking", BlockingStatus::KnownBlocking);
        let root_lexically_first_but_longer =
            external(&mut graph, "0.blocking", BlockingStatus::KnownBlocking);
        call(&mut graph, start, mid, false, 15);
        call(&mut graph, mid, root_lexically_first_but_longer, false, 16);
        call(&mut graph, start, root_b, false, 30);
        call(&mut graph, start, root_a, false, 25);
        call(&mut graph, start, root_a, false, 20);

        let result = propagate_blocking(&mut graph);

        let reason = &result.blocking_reasons[&start];
        assert_eq!(reason.root_cause, root_a);
        assert_eq!(reason.chain_links.len(), 1);
        assert_eq!(reason.chain_links[0].callee_name, "a.blocking");
        assert_eq!(reason.chain_links[0].call_site_location, Some(loc(20)));
    }
}
