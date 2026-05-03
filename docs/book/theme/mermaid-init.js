window.addEventListener('DOMContentLoaded', function () {
    if (!window.mermaid) {
        return;
    }

    window.mermaid.initialize({
        startOnLoad: true,
        securityLevel: 'loose',
        theme: document.documentElement.classList.contains('dark') ? 'dark' : 'default'
    });
});
