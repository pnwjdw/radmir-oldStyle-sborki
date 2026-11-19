(function () {
    if (!window.__oldDialogsStyles) {
        const records = new Map();
        let sequence = 0;
        function selectors(source) {
            const parts = [];
            let start = 0, depth = 0, quote = '';
            for (let i = 0; i < source.length; ++i) {
                const c = source[i];
                if (c === '\\') { ++i; continue; }
                if (quote) { if (c === quote) quote = ''; continue; }
                if (c === '"' || c === "'") quote = c;
                else if (c === '(' || c === '[') ++depth;
                else if (c === ')' || c === ']') --depth;
                else if (c === ',' && !depth) { parts.push(source.slice(start, i)); start = i + 1; }
            }
            parts.push(source.slice(start));
            return parts;
        }
        function scoped(source, prefix) {
            let output = '', start = 0, depth = 0, quote = '', header = '', body = 0;
            for (let i = 0; i < source.length; ++i) {
                const c = source[i];
                if (quote) {
                    if (c === '\\') ++i;
                    else if (c === quote) quote = '';
                    continue;
                }
                if (c === '"' || c === "'") { quote = c; continue; }
                if (c === '/' && source[i + 1] === '*') {
                    const end = source.indexOf('*/', i + 2);
                    i = end < 0 ? source.length : end + 1;
                    continue;
                }
                if (!depth && c === ';') { output += source.slice(start, i + 1); start = i + 1; continue; }
                if (c === '{') {
                    if (!depth) {
                        header = source.slice(start, i).replace(/^\s*(?:\/\*[\s\S]*?\*\/\s*)*/, '').trim();
                        body = i + 1;
                    }
                    ++depth;
                } else if (c === '}' && depth && !--depth) {
                    const content = source.slice(body, i);
                    if (/^@(media|supports)\b/i.test(header)) output += header + '{' + scoped(content, prefix) + '}';
                    else if (header[0] === '@') output += header + '{' + content + '}';
                    else if (header) {
                        const rules = selectors(header).map((value) => {
                            const selector = value.trim();
                            if (!selector) return '';
                            const root = selector.match(/^(?::root|html|:scope)(?=$|[\s.#:\[>+~])/i);
                            return root ? prefix + selector.slice(root[0].length) : prefix + ' ' + selector;
                        }).filter(Boolean);
                        if (rules.length) output += rules.join(',') + '{' + content + '}';
                    }
                    start = i + 1;
                }
            }
            return output;
        }
        window.__oldDialogsStyles = {
            set(id, css) {
                id = String(id);
                css = String(css || '');
                const previous = records.get(id),
                    old = previous ? previous.node : document.getElementById(id);
                if (previous && previous.css === css && old && old.isConnected !== false && old.parentNode &&
                    document.documentElement.getAttribute(previous.attribute) === previous.revision)
                    return old;
                const attribute = previous ? previous.attribute : 'data-od-sheet-' + (++sequence),
                    revision = String(++sequence),
                    prefix = 'html[' + attribute + '="' + revision + '"]',
                    node = document.createElement('style');
                node.id = id;
                node.textContent = scoped(css, prefix);
                (document.head || document.documentElement).appendChild(node);
                document.documentElement.setAttribute(attribute, revision);
                if (old && old !== node && old.parentNode) old.parentNode.removeChild(old);
                records.set(id, { node, css, attribute, revision });
                return node;
            },
            remove(id) {
                id = String(id);
                const previous = records.get(id),
                    node = previous ? previous.node : document.getElementById(id);
                if (previous) document.documentElement.removeAttribute(previous.attribute);
                if (node && node.parentNode) node.parentNode.removeChild(node);
                records.delete(id);
            }
        };
    }
    if (window.__oldDialogsHassle && window.__oldDialogsHassle.version === 11)
        return window.__oldDialogsHassle;
    if (window.__oldDialogsHassle && window.__oldDialogsHassle.shutdown) window.__oldDialogsHassle.shutdown();
    const scope = '#app [data-od-dialog]';
    const state = {
        config: null,
        base: '',
        style: null,
        fonts: new Map(),
        fontChunks: new Map(),
        expired: false,
        seen: Date.now(),
        timer: 0,
        disposed: false,
        dialogs: [],
        titles: new Map()
    };
    function readApplied() {
        function node(element) {
            if (!element) return null;
            try {
                const css = getComputedStyle(element), box = element.getBoundingClientRect();
                return {
                    display: css.display, visibility: css.visibility, opacity: css.opacity,
                    font: css.fontFamily, fontSize: css.fontSize, lineHeight: css.lineHeight,
                    weight: css.fontWeight, color: css.color, background: css.backgroundColor,
                    position: css.position, transform: css.transform,
                    x: box.left, y: box.top, width: box.width, height: box.height,
                    cssWidth: css.width, cssHeight: css.height
                };
            } catch (error) { return { error: String(error) }; }
        }
        function pick(selector, root) {
            try { return node((root || document).querySelector(selector)); }
            catch (error) { return { error: String(error) }; }
        }
        function selected(source, keys) {
            const result = {};
            if (!source || typeof source !== 'object') return result;
            for (const key of keys) {
                const value = source[key];
                if (typeof value === 'boolean' || (typeof value === 'number' && isFinite(value)))
                    result[key] = value;
            }
            return result;
        }
        let hud = null;
        try { hud = typeof getInterface === 'function' ? getInterface('Hud') : null; } catch (_) {}
        const attributes = {};
        try {
            Array.prototype.forEach.call(document.documentElement.attributes, function (attribute) {
                if (/^data-od-sheet-/.test(attribute.name)) attributes[attribute.name] = attribute.value;
            });
        } catch (_) {}
        let dialogs = [], styles = [];
        try {
            dialogs = Array.prototype.slice.call(document.querySelectorAll('[data-od-dialog]'), 0, 12).map(function (element) {
                return {
                    name: element.getAttribute('data-od-dialog'), kind: element.getAttribute('data-od-kind'),
                    outer: node(element), container: pick('.modal-container', element),
                    header: pick('.modal__title', element), body: pick('.window__content', element),
                    row: pick('.window-table__item', element), cell: pick('.window-table__col', element),
                    cellText: pick('.window-table__col p,.window-table__col span', element),
                    input: pick('input,textarea', element), button: pick('.window__buttons .controls-button', element)
                };
            });
        } catch (_) {}
        try {
            styles = Array.prototype.map.call(document.querySelectorAll('style[id^=__oldDialogs]'), function (style) {
                return { id: style.id, length: style.textContent.length, attached: !!style.parentNode };
            });
        } catch (_) {}
        const fonts = { supported: false };
        try {
            fonts.supported = !!document.fonts;
            if (document.fonts) fonts.status = document.fonts.status;
        } catch (_) {}
        const config = state.config || {}, widgets = config.widgets || {};
        return {
            sampledAt: Date.now(), viewport: { width: innerWidth, height: innerHeight },
            styleAttrs: attributes, styleNodes: styles, fonts: fonts, dialogs: dialogs,
            expected: {
                enabled: !!config.enabled, fontFamily: config.fontFamily, bold: !!config.bold,
                sizes: selected(config.sizes, ['scale', 'font', 'titleFont', 'listFont', 'inputFont', 'windowWidth', 'dialogWidth', 'rowHeight', 'headerHeight', 'buttonHeight', 'padding', 'borderWidth', 'radius']),
                chat: selected(widgets.chat, ['enabled', 'input', 'resize', 'font', 'bold', 'lines', 'size', 'lineHeight', 'width', 'x', 'y']),
                logo: selected(widgets.logo, ['style', 'font', 'width', 'height', 'scale', 'right', 'top', 'followHud', 'customMode', 'customActive']),
                radar: selected(widgets.radar, ['enabled', 'width', 'height', 'x', 'bottom', 'shape']),
                safety: selected(widgets.safety, ['enabled', 'size', 'x', 'bottom', 'followRadar'])
            },
            chat: {
                root: pick('.radmir-chat'), messages: pick('.radmir-chat__messages'),
                text: pick('.chat-message-content'), input: pick('.radmir-chat input,.radmir-chat textarea')
            },
            logo: {
                root: pick('#__oldDialogsLogo'), title: pick('#__oldDialogsLogo .odl-title'),
                subtitle: pick('#__oldDialogsLogo .odl-subtitle'), mark: pick('#__oldDialogsLogo .odl-mark'),
                native: pick('.hud-radmir-info')
            },
            radar: pick('.hud-radmir-radar__radar'), safety: pick('.hud-radmir-radar__safe-zone'),
            speedometer: {
                native: pick('[class*=speedometer],[class*=speedmetr]'), custom: pick('#__oldDialogsSpeedometer'),
                available: !!(hud && hud.speedometer),
                state: selected(hud && hud.speedometer, ['show', 'speed', 'fuel', 'maxFuel', 'maxSpeed'])
            },
            hud: hud ? {
                infoShown: !!(hud.info && hud.info.show), barsShown: !!(hud.info && hud.info.showBars),
                radarShown: !!(hud.radar && hud.radar.show), greenZone: !!(hud.radar && hud.radar.greenZone),
                logoShown: !!hud.isShowLogo, manuallyHidden: !!hud.hudIsManuallyHidden
            } : null
        };
    }
    let appliedSnapshot = null, appliedPending = false, appliedEpoch = 0;
    function queueApplied() {
        if (state.disposed || appliedPending) return;
        if (typeof requestAnimationFrame !== 'function') {
            appliedSnapshot = { sampledAt: Date.now(), error: 'requestAnimationFrame is unavailable' };
            return;
        }
        appliedPending = true;
        const epoch = appliedEpoch;
        requestAnimationFrame(function () {
            requestAnimationFrame(function () {
                if (epoch !== appliedEpoch || state.disposed) return;
                try { appliedSnapshot = readApplied(); }
                catch (error) { appliedSnapshot = { sampledAt: Date.now(), error: String(error) }; }
                appliedPending = false;
            });
        });
    }
    function resetApplied() {
        appliedEpoch += 1;
        appliedSnapshot = null;
        appliedPending = false;
        queueApplied();
    }
    function appliedStatus() {
        queueApplied();
        return Object.assign({
            ready: !!(appliedSnapshot && !appliedSnapshot.error), queued: appliedPending,
            ageMs: appliedSnapshot ? Date.now() - appliedSnapshot.sampledAt : null
        }, appliedSnapshot || {});
    }
    function restoreTitles(active) {
        for (const [node, item] of state.titles)
            if (!active || !active.has(node)) {
                if (node.isConnected && node.innerHTML === item.applied) node.innerHTML = item.original;
                state.titles.delete(node);
            }
    }
    function coloredTitle(source) {
        const escape = (text) =>
            text
                .replace(/&/g, '&amp;')
                .replace(/</g, '&lt;')
                .replace(/>/g, '&gt;')
                .replace(/"/g, '&quot;')
                .replace(/'/g, '&#39;');
        const parts = source.split(/(\{[a-f\d]{6}(?:[a-f\d]{2})?\})/gi);
        let color = '',
            result = '';
        for (const part of parts) {
            if (/^\{[a-f\d]{6}(?:[a-f\d]{2})?\}$/i.test(part)) color = '#' + part.slice(1, -1);
            else if (part)
                result += color
                    ? '<span style="color:' + color + '">' + escape(part) + '</span>'
                    : escape(part);
        }
        return result;
    }
    function split(source, delimiter) {
        const result = [];
        let quote = '',
            depth = 0,
            start = 0;
        for (let i = 0; i < source.length; ++i) {
            const c = source[i];
            if (quote) {
                if (c === '\\') ++i;
                else if (c === quote) quote = '';
                continue;
            }
            if (c === '"' || c === "'") quote = c;
            else if (c === '(' || c === '[') ++depth;
            else if (c === ')' || c === ']') --depth;
            else if (!depth && c === delimiter) {
                result.push(source.slice(start, i));
                start = i + 1;
            }
        }
        result.push(source.slice(start));
        return result;
    }
    function scopedCss(source) {
        let output = '',
            start = 0,
            depth = 0,
            quote = '',
            header = '',
            body = 0;
        for (let i = 0; i < source.length; ++i) {
            const c = source[i];
            if (quote) {
                if (c === '\\') ++i;
                else if (c === quote) quote = '';
                continue;
            }
            if (c === '"' || c === "'") {
                quote = c;
                continue;
            }
            if (c === '/' && source[i + 1] === '*') {
                const end = source.indexOf('*/', i + 2);
                i = end < 0 ? source.length : end + 1;
                continue;
            }
            if (!depth && c === ';') {
                start = i + 1;
                continue;
            }
            if (c === '{') {
                if (!depth) {
                    header = source
                        .slice(start, i)
                        .replace(/\/\*[\s\S]*?\*\//g, '')
                        .trim();
                    body = i + 1;
                }
                ++depth;
            } else if (c === '}' && depth && !--depth) {
                const content = source.slice(body, i);
                if (/^@(media|supports)\b/i.test(header)) output += header + '{' + scopedCss(content) + '}';
                else if (header && header[0] !== '@') {
                    const selectors = split(header, ',')
                        .map((x) => x.trim())
                        .filter(Boolean)
                        .map((selector) => {
                            if (/^(?::dialog|\.window|:root|html|body|#app)$/.test(selector)) return scope;
                            if (/html|body|#app|:root|:scope|:dialog/i.test(selector)) return null;
                            if (/^\.window\s+/.test(selector))
                                selector = selector.replace(/^\.window\s+/, '');
                            if (/^[+~]/.test(selector)) return null;
                            return scope + ' ' + selector;
                        })
                        .filter(Boolean);
                    if (selectors.length) output += selectors.join(',') + '{' + content + '}';
                }
                start = i + 1;
            }
        }
        return output;
    }
    function removeMarkers() {
        for (const node of document.querySelectorAll('[data-od-dialog],[data-od-panel],[data-od-surface]')) {
            node.removeAttribute('data-od-dialog');
            node.removeAttribute('data-od-panel');
            node.removeAttribute('data-od-surface');
            node.removeAttribute('data-od-kind');
            node.removeAttribute('data-od-colors');
            node.removeAttribute('data-od-animations');
        }
    }
    const panelDefinitions = {
        CasinoDice: ['Игра в кости', '.dice-window,.dice-modal'],
        Trade: ['Обмен', '.trade,.modal'],
        TradeItems: [
            'Обмен предметами',
            '.trade-items__main,.trade-items-exchange__container,.trade-items,.trade,.modal'
        ],
        TradeSkin: ['Обмен одеждой', '.trade-skin__main,.trade-skin,.modal'],
        CasinoExchange: ['Обмен фишек', '.casino-exchange,.exchange,.modal'],
        Invitation: ['Приглашение', '.invitation,.modal'],
        Invite: ['Приглашение', '.invite,.modal'],
        Offer: ['Предложение', '.offer,.modal'],
        BuyCarNumber: ['Покупка номера', '.buy-car-number,.modal'],
        CarKey: ['Ключи транспорта', '.car-key,.modal'],
        InfoCard: ['Информация', '.info-card,.modal'],
        ActorDialog: ['Диалог', '.actor-dialog,.modal']
    };
    function scanPanels(components, dialogs) {
        for (const [name, definition] of Object.entries(panelDefinitions)) {
            if (!components[name]) continue;
            let root = null;
            try {
                const instance = window.getInterface && window.getInterface(name);
                root = instance && instance.$el;
            } catch (_) {}
            if (!root || root.nodeType !== 1 || !root.isConnected) continue;
            const surfaces = Array.from(root.querySelectorAll(definition[1]));
            if (typeof root.matches === 'function' && root.matches(definition[1])) surfaces.unshift(root);
            if (!surfaces.length)
                surfaces.push(
                    root.classList.contains('iface-centered') && root.firstElementChild
                        ? root.firstElementChild
                        : root
                );
            dialogs.push({ name, title: definition[0], kind: 'panel', opened: true, loaded: true });
            if (!state.config || !state.config.enabled) continue;
            const markers = {
                'data-od-panel': name,
                'data-od-colors': state.config.preserveColors ? 'keep' : 'replace',
                'data-od-animations': state.config.animations ? 'keep' : 'off'
            };
            for (const [key, value] of Object.entries(markers))
                if (root.getAttribute(key) !== value) root.setAttribute(key, value);
            for (const surface of surfaces)
                if (!surface.hasAttribute('data-od-surface')) surface.setAttribute('data-od-surface', '');
        }
    }
    function scan() {
        state.timer = 0;
        if (state.disposed) return;
        const components = (window.App && window.App.components) || {},
            active = new Set(),
            activeTitles = new Set(),
            dialogs = [];
        for (const name of Object.keys(components)) {
            const component = components[name];
            if (!/^Window\d+$/.test(name) || !component.isDialog) continue;
            const params = (component.open && component.open.params) || [];
            const kind =
                ['text', 'input', 'list', 'password', 'table', 'table', 'image'][params[1]] || 'text';
            let dialog = null;
            try {
                const instance = window.getInterface && window.getInterface(name),
                    root = instance && instance.$el;
                if (root && root.nodeType === 1)
                    dialog = root.classList.contains('window') ? root : root.querySelector('.window');
            } catch (_) {}
            dialogs.push({
                name: name,
                title: String(params[2] || '').replace(/\{[a-f\d]{6,8}\}/gi, ''),
                kind: kind,
                opened: !!dialog,
                loaded: !!dialog
            });
            if (!dialog || !state.config || !state.config.enabled) continue;
            active.add(dialog);
            if (dialog.getAttribute('data-od-dialog') !== name) dialog.setAttribute('data-od-dialog', name);
            if (dialog.getAttribute('data-od-kind') !== kind) dialog.setAttribute('data-od-kind', kind);
            const colors = state.config.preserveColors ? 'keep' : 'replace';
            if (dialog.getAttribute('data-od-colors') !== colors)
                dialog.setAttribute('data-od-colors', colors);
            const animations = state.config.animations ? 'keep' : 'off';
            if (dialog.getAttribute('data-od-animations') !== animations)
                dialog.setAttribute('data-od-animations', animations);
            const title = dialog.querySelector('.modal__title'),
                source = String(params[2] || '');
            if (
                title &&
                state.config.preserveColors &&
                /\{[a-f\d]{6}(?:[a-f\d]{2})?\}/i.test(source) &&
                !/[<>]/.test(source)
            ) {
                activeTitles.add(title);
                let item = state.titles.get(title);
                if (!item) {
                    item = { original: title.innerHTML, source: '', applied: '' };
                    state.titles.set(title, item);
                } else if (title.innerHTML !== item.applied) item.original = title.innerHTML;
                if (item.source !== source || title.innerHTML !== item.applied) {
                    title.innerHTML = coloredTitle(source);
                    item.source = source;
                    item.applied = title.innerHTML;
                }
            }
        }
        for (const node of document.querySelectorAll('[data-od-dialog]'))
            if (!active.has(node)) {
                if (
                    state.config &&
                    state.config.enabled &&
                    node.isConnected &&
                    node.classList.contains('window')
                ) {
                    const title = node.querySelector('.modal__title');
                    if (title) activeTitles.add(title);
                    continue;
                }
                node.removeAttribute('data-od-dialog');
                node.removeAttribute('data-od-kind');
                node.removeAttribute('data-od-colors');
                node.removeAttribute('data-od-animations');
            }
        scanPanels(components, dialogs);
        restoreTitles(activeTitles);
        state.dialogs = dialogs;
    }
    function removeStyle() {
        window.__oldDialogsStyles.remove('__oldDialogsHassleStyle');
        state.style = null;
        restoreTitles();
        removeMarkers();
    }
    function rebuild() {
        if (!state.config || !state.config.enabled) {
            removeStyle();
            scan();
            return;
        }
        const cfg = state.config,
            size = cfg.sizes;
        const values = {};
        let css = scope + ',#app [data-od-panel]{';
        for (const key of Object.keys(cfg.colors)) {
            const c = cfg.colors[key];
            values[key] =
                'rgba(' +
                c
                    .slice(0, 3)
                    .map((x) => Math.round(x * 255))
                    .join(',') +
                ',' +
                c[3] +
                ')';
            css += '--od-' + key + ':' + values[key] + ';';
        }
        for (const key of Object.keys(size)) {
            values[key] =
                +(size[key] * (key === 'scale' ? 1 : size.scale)).toFixed(3) + (key === 'scale' ? '' : 'px');
            css += '--od-' + key + ':' + values[key] + ';';
        }
        values.fontFamily = 'OldDialogsLocal11_' + cfg.fontFamily + ',"Hassle Open Sans","Roboto"';
        values.weight = cfg.bold ? '700' : '400';
        values.listBorderWidth = size.borderWidth * size.scale * 2 + 'px';
        css += '--od-fontFamily:' + values.fontFamily + ';--od-weight:' + values.weight + ';}';
        css += state.base.replace(
            /(-?(?:\d*\.)?\d+)px\b/g,
            (_, number) => +(Number(number) * size.scale).toFixed(3) + 'px'
        );
        if (size.dialogWidth > 0)
            css += scope + ' .modal-container-wrapper{width:var(--od-dialogWidth)!important;}';
        if (size.windowHeight > 0)
            css +=
                scope +
                ' .modal-container-wrapper{max-height:var(--od-windowHeight)!important;overflow:auto!important;}';
        css += scopedCss(cfg.customCss || '');
        css = css.replace(/var\(--od-([A-Za-z]+)\)/g, (match, name) =>
            values[name] === undefined ? match : values[name]
        );
        state.style = window.__oldDialogsStyles.set('__oldDialogsHassleStyle', css);
        scan();
    }
    function schedule() {
        if (!state.timer && !state.disposed) state.timer = setTimeout(scan, 30);
    }
    const observer = new MutationObserver((records) => {
        for (const record of records) {
            if (
                record.target.nodeType === 1 &&
                record.target.closest &&
                record.target.closest('[data-od-dialog]')
            ) {
                schedule();
                return;
            }
            for (const node of record.addedNodes)
                if (node.nodeType === 1 && node.tagName !== 'STYLE' && node.tagName !== 'SCRIPT') {
                    schedule();
                    return;
                }
            if (record.removedNodes.length) schedule();
        }
    });
    const app = document.getElementById('app');
    if (app) observer.observe(app, { childList: true, subtree: true });
    const lease = setInterval(() => {
        if (state.config && state.config.enabled && Date.now() - state.seen > 15000) {
            state.expired = true;
            state.config.enabled = false;
            removeStyle();
        }
    }, 1000);
    const api = {
        version: 11,
        fontChunk(key, chunk, reset, commit) {
            state.seen = Date.now();
            key = String(key);
            if (!/^[0-9]{1,5}$/.test(key) || typeof chunk !== 'string' || chunk.length > 262144)
                throw new Error('Invalid font upload');
            const value = (reset ? '' : state.fontChunks.get(key) || '') + chunk;
            if (value.length > 201326592) throw new Error('Font upload exceeds 16 MiB');
            if (!commit) {
                state.fontChunks.set(key, value);
                return {};
            }
            const style = window.__oldDialogsStyles.set('__oldDialogsHassleFont' + key, value);
            state.fonts.set(key, style);
            state.fontChunks.delete(key);
            return {};
        },
        configure(config, base, fonts) {
            state.expired = false;
            state.seen = Date.now();
            state.config = config;
            if (typeof base === 'string') state.base = base;
            if (fonts && typeof fonts === 'object')
                for (const key of Object.keys(fonts)) {
                    const style = window.__oldDialogsStyles.set('__oldDialogsHassleFont' + key, fonts[key]);
                    state.fonts.set(key, style);
                }
            rebuild();
            resetApplied();
            return api.status();
        },
        status() {
            state.seen = Date.now();
            scan();
            const hud = window.__oldDialogsHud ? window.__oldDialogsHud.status() : {};
            const layout = window.__oldDialogsLayout ? window.__oldDialogsLayout.status() : {};
            return {
                version: 11,
                leaseExpired: state.expired || !!hud.expired,
                enabled: !!(state.config && state.config.enabled),
                styles: state.style ? 1 : 0,
                rules: state.style ? (state.style.textContent.match(/\{/g) || []).length : 0,
                pending: 0,
                failed: 0,
                windows: state.dialogs.length,
                interfaces: state.dialogs,
                hud: hud,
                layout: layout,
                applied: appliedStatus()
            };
        },
        shutdown() {
            state.disposed = true;
            appliedEpoch += 1;
            appliedSnapshot = null;
            appliedPending = false;
            clearInterval(lease);
            clearTimeout(state.timer);
            observer.disconnect();
            removeStyle();
            for (const key of state.fonts.keys())
                window.__oldDialogsStyles.remove('__oldDialogsHassleFont' + key);
            state.fonts.clear();
            state.fontChunks.clear();
            if (window.__oldDialogsHud) window.__oldDialogsHud.shutdown();
            if (window.__oldDialogsLayout) window.__oldDialogsLayout.shutdown();
            delete window.__oldDialogsHassle;
            return true;
        }
    };
    window.__oldDialogsHassle = api;
    return api;
})();
