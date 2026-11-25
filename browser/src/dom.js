(() => {
    const query = globalThis.__query;
    const getId = globalThis.__getId;
    const getText = globalThis.__getText;
    const setText = globalThis.__setText;
    delete globalThis.__query;
    delete globalThis.__getId;
    delete globalThis.__getText;
    delete globalThis.__setText;
    const elements = new Map();
    const ready = [];

    function element(id) {
        if (id == null) return null;
        if (elements.has(id)) return elements.get(id);
        const listeners = new Set();
        const node = {
            get textContent() { return getText(id); },
            set textContent(value) { setText(id, value == null ? "" : String(value)); },
            onclick: null,
            addEventListener(type, listener) {
                if (type === "click" && typeof listener === "function") listeners.add(listener);
            },
            removeEventListener(type, listener) {
                if (type === "click") listeners.delete(listener);
            },
        };
        elements.set(id, node);
        Object.defineProperty(node, "__click", { value(event) {
            if (typeof node.onclick === "function" && node.onclick(event) === false) {
                event.preventDefault();
            }
            for (const listener of listeners) listener.call(node, event);
        } });
        return node;
    }

    globalThis.window = globalThis;
    globalThis.document = {
        readyState: "loading",
        querySelector: (selector) => element(query(String(selector))),
        getElementById: (id) => element(getId(String(id))),
        addEventListener(type, listener) {
            if (type === "DOMContentLoaded" && typeof listener === "function") ready.push(listener);
        },
    };
    globalThis.__makerReady = () => {
        document.readyState = "complete";
        for (const listener of ready.splice(0)) {
            try { listener.call(document, { type: "DOMContentLoaded" }); } catch (_) {}
        }
    };
    globalThis.__makerClick = (path) => {
        let cancelled = false;
        let stopped = false;
        const event = {
            type: "click",
            target: element(path[0]),
            currentTarget: null,
            preventDefault() { cancelled = true; },
            stopPropagation() { stopped = true; },
        };
        for (const id of path) {
            const node = elements.get(id);
            if (node) {
                event.currentTarget = node;
                try { node.__click(event); } catch (_) {}
            }
            if (stopped) break;
        }
        return cancelled;
    };
})();
