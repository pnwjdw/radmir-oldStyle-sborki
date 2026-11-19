(function () {
    if (window.__oldDialogsLayout) window.__oldDialogsLayout.shutdown();
    const state = {
        config: {},
        widgets: {},
        dpi: 1,
        nodes: new Map(),
        style: null,
        css: '',
        radar: '',
        active: new Set(),
        restoring: new Map(),
        disposed: false
    };
    const selectors = [
        ['radar', 'Радар', '.hud-radmir-radar__radar'],
        ['safety', 'Безопасная зона', '.hud-radmir-radar__safe-zone'],
        ['chat', 'Чат', '.radmir-chat'],
        ['logo', 'Логотип', '#__oldDialogsLogo'],
        ['hud', 'HUD целиком', '.hud-radmir-info,.hud-hassle-info'],
        ['message', 'Сообщение по центру', '.game-text--content.center-type'],
        ['message.top', 'Сообщение сверху', '.game-text--content.top-type'],
        ['message.bottom', 'Сообщение снизу', '.game-text--content.bottom-type'],
        ['message.key', 'Подсказка действия', '.game-text--content.key-type'],
        ['message.money', 'Изменение денег', '.game-text--content.right-type']
    ];
    function refreshRadar() {
        if (!state.disposed && window.__oldDialogsHud && window.__oldDialogsHud.layoutChanged)
            window.__oldDialogsHud.layoutChanged();
    }
    function collect() {
        if (state.disposed) return;
        const candidates = [];
        for (const [id, label, selector] of selectors) {
            const node = document.querySelector(selector);
            if (node) candidates.push({ id, label, node });
        }
        for (const node of document.querySelectorAll('[data-od-dialog],[data-od-panel]')) {
            const name = node.getAttribute('data-od-dialog') || node.getAttribute('data-od-panel');
            candidates.push({ id: 'dialog.' + name, label: 'Окно: ' + name, node });
        }
        const active = new Set();
        let css = '',
            radar = '';
        const transformed = new Set();
        for (const candidate of candidates) {
            const { id, node } = candidate;
            if (!node.isConnected) continue;
            active.add(id);
            let item = state.nodes.get(id);
            if (!item || item.node !== node) {
                if (item) item.node.removeAttribute('data-od-layout');
                const transform = getComputedStyle(node).transform || '';
                item = { ...candidate, base: transform === 'none' ? '' : transform };
                state.nodes.set(id, item);
            }
            if (node.getAttribute('data-od-layout') !== id) node.setAttribute('data-od-layout', id);
            let value = state.config[id];
            const logo = state.widgets.logo;
            if (
                id === 'logo' &&
                logo &&
                logo.followHud &&
                state.widgets.playerHud &&
                state.widgets.playerHud.style &&
                state.config.hud
            ) {
                const own = value || { x: 0, y: 0, scale: 1 };
                value = { ...own, x: own.x + state.config.hud.x, y: own.y + state.config.hud.y };
            }
            let base = item.base;
            if (id === 'logo' && logo)
                base =
                    'scale(' +
                    ((logo.width || 252) / 252) * logo.scale * state.dpi +
                    ',' +
                    ((logo.height || 64) / 64) * logo.scale * state.dpi +
                    ')';
            if (!value || (!value.x && !value.y && value.scale === 1)) {
                if (state.active.has(id)) state.restoring.set(id, Date.now() + 75);
                if ((state.restoring.get(id) || 0) > Date.now())
                    css +=
                        '#app [data-od-layout][data-od-layout="' +
                        id +
                        '"]{transform:' +
                        (base || 'none') +
                        '!important;transition:none!important;}';
                else state.restoring.delete(id);
                continue;
            }
            transformed.add(id);
            state.restoring.delete(id);
            const transform =
                'translate3d(' +
                value.x * state.dpi +
                'px,' +
                value.y * state.dpi +
                'px,0) scale(' +
                value.scale +
                ')' +
                (base ? ' ' + base : '');
            const rule =
                '#app [data-od-layout][data-od-layout="' +
                id +
                '"]{transform:' +
                transform +
                '!important;transition-property:opacity!important;}';
            css += rule;
            if (id === 'radar') radar = rule;
        }
        for (const [id, item] of state.nodes)
            if (!active.has(id)) {
                item.node.removeAttribute('data-od-layout');
                state.nodes.delete(id);
            }
        state.active = transformed;
        if (state.restoring.size) setTimeout(collect, 90);
        if (css !== state.css) {
            state.css = css;
            if (!css) {
                window.__oldDialogsStyles.remove('__oldDialogsLayoutStyle');
                state.style = null;
            } else {
                state.style = window.__oldDialogsStyles.set('__oldDialogsLayoutStyle', css);
            }
        }
        if (radar !== state.radar) {
            state.radar = radar;
            setTimeout(refreshRadar, 50);
        }
    }
    const api = {
        configure(config, dpi, widgets) {
            state.config = config || {};
            state.widgets = widgets || {};
            state.dpi = dpi || 1;
            collect();
            return {};
        },
        status() {
            collect();
            const elements = [];
            for (const item of state.nodes.values()) {
                const style = getComputedStyle(item.node);
                if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0)
                    continue;
                let rect = item.node.getBoundingClientRect();
                const surfaces = Array.from(
                    item.node.querySelectorAll('[data-od-surface],.modal-container-wrapper')
                )
                    .map((node) => node.getBoundingClientRect())
                    .filter((r) => r.width > 2 && r.height > 2);
                if (surfaces.length && item.id.startsWith('dialog.')) {
                    const left = Math.min(...surfaces.map((r) => r.left)),
                        top = Math.min(...surfaces.map((r) => r.top));
                    const right = Math.max(...surfaces.map((r) => r.right)),
                        bottom = Math.max(...surfaces.map((r) => r.bottom));
                    rect = { left, top, width: right - left, height: bottom - top };
                }
                if (rect.width > 2 && rect.height > 2)
                    elements.push({
                        id: item.id,
                        label: item.label,
                        x: rect.left,
                        y: rect.top,
                        width: rect.width,
                        height: rect.height
                    });
            }
            return { width: window.innerWidth, height: window.innerHeight, elements };
        },
        shutdown() {
            state.disposed = true;
            window.__oldDialogsStyles.remove('__oldDialogsLayoutStyle');
            for (const item of state.nodes.values()) item.node.removeAttribute('data-od-layout');
            state.nodes.clear();
            delete window.__oldDialogsLayout;
        }
    };
    window.__oldDialogsLayout = api;
    return api;
})();
