(function () {
    if (window.__oldDialogsHud && window.__oldDialogsHud.version === 4) return window.__oldDialogsHud;
    if (window.__oldDialogsHud) window.__oldDialogsHud.shutdown();
    const state = {
        config: null,
        style: null,
        css: '',
        timer: 0,
        disposed: false,
        expired: false,
        seen: Date.now(),
        labels: new Map(),
        radar: null,
        safety: null,
        chat: null,
        chatRows: null,
        preview: null,
        previewTimer: 0
    };
    const color = (c) =>
        'rgba(' +
        c
            .slice(0, 3)
            .map((x) => Math.round(x * 255))
            .join(',') +
        ',' +
        c[3] +
        ')';
    const font = (n) => 'OldDialogsLocal11_' + n + ',"Hassle Open Sans","Roboto"';
    const px = (n) => Number(n).toFixed(3) + 'px';
    const hue = (c) => {
        const max = Math.max(c[0], c[1], c[2]),
            min = Math.min(c[0], c[1], c[2]),
            delta = max - min;
        if (!delta) return 0;
        const value =
            max === c[0]
                ? (c[1] - c[2]) / delta
                : max === c[1]
                  ? (c[2] - c[0]) / delta + 2
                  : (c[0] - c[1]) / delta + 4;
        return (value * 60 + 360) % 360;
    };
    function shadow(width, rgba) {
        if (!width) return 'none';
        return [
            [-1, -1],
            [0, -1],
            [1, -1],
            [-1, 0],
            [1, 0],
            [-1, 1],
            [0, 1],
            [1, 1]
        ]
            .map((p) => px(p[0] * width) + ' ' + px(p[1] * width) + ' 0 ' + color(rgba))
            .join(',');
    }
    function clearMarker(name) {
        for (const node of document.querySelectorAll('[' + name + ']')) node.removeAttribute(name);
    }
    function restoreLabels(active) {
        for (const [node, item] of state.labels)
            if (!active || !active.has(node)) {
                if (node.isConnected && node.nodeValue === item.applied) node.nodeValue = item.original;
                state.labels.delete(node);
            }
    }
    function text(label, value, active) {
        const node = Array.from(label.childNodes).find(
            (item) => item.nodeType === 3 && (item.nodeValue.trim() || state.labels.has(item))
        );
        if (!node) return;
        active.add(node);
        let item = state.labels.get(node);
        if (!item) {
            item = { original: node.nodeValue, applied: null };
            state.labels.set(node, item);
        } else if (node.nodeValue !== item.applied) item.original = node.nodeValue;
        if (node.nodeValue !== value) node.nodeValue = value;
        item.applied = value;
    }
    function restoreChat() {
        if (state.chatRows && window.App && window.App.$data) {
            if (window.App.$data.chatPageSize === state.chatRows.applied)
                window.App.$data.chatPageSize = state.chatRows.original;
            state.chatRows = null;
        }
    }
    function pixels(value, fallback) {
        const number = parseFloat(value);
        if (!Number.isFinite(number)) return fallback;
        if (/vh$/.test(value)) return (number * window.innerHeight) / 100;
        if (/vw$/.test(value)) return (number * window.innerWidth) / 100;
        return number;
    }
    function viewportAnchor(element, left, bottom) {
        const parent = element.parentElement;
        if (!parent) return { left, bottom };
        const bounds = parent.getBoundingClientRect();
        return {
            left: left - bounds.left,
            bottom: bottom - (window.innerHeight - bounds.bottom)
        };
    }
    function sync() {
        state.timer = 0;
        if (state.disposed || !state.config) return;
        const cfg = state.config,
            radar = cfg.radar,
            safety = cfg.safety,
            chat = cfg.chat;
        state.radar = document.querySelector('.hud-radmir-radar__radar');
        state.safety = document.querySelector('.hud-radmir-radar__safe-zone');
        state.chat = document.querySelector('.radmir-chat');
        let css = '';
        const labels = new Set();
        if (radar.enabled && state.radar) {
            const element = state.radar,
                native = element.querySelector('.hud-hassle-map');
            if (native) {
                const base = getComputedStyle(native),
                    w = Math.max(1, pixels(base.width, native.offsetWidth)),
                    h = Math.max(1, pixels(base.height, native.offsetHeight));
                const width = radar.width,
                    height = radar.shape < 2 ? radar.width : radar.height,
                    radius = radar.shape === 1 || radar.shape === 2 ? '50%' : px(radar.rounding || 0);
                const sx = width / w,
                    sy = height / h;
                if (!element.hasAttribute('data-od-radar')) element.setAttribute('data-od-radar', '');
                const r = '#app [data-od-radar]';
                const anchor = viewportAnchor(element, radar.x, radar.bottom);
                const animatedBorder = radar.effect && radar.effectTarget !== 1;
                css +=
                    r +
                    '{position:absolute!important;left:' +
                    px(anchor.left) +
                    '!important;bottom:' +
                    px(anchor.bottom) +
                    '!important;top:auto!important;right:auto!important;width:' +
                    px(width) +
                    '!important;height:' +
                    px(height) +
                    '!important;margin:0!important;padding:0!important;box-sizing:content-box!important;border-width:' +
                    px(radar.border) +
                    '!important;border-style:solid!important;border-color:' +
                    color(radar.borderColor) +
                    (animatedBorder ? '' : '!important') +
                    ';border-radius:' +
                    radius +
                    '!important;overflow:hidden!important;opacity:' +
                    radar.opacity +
                    '!important;background-color:' +
                    color(radar.background) +
                    '!important;}';
                css +=
                    r +
                    '>.hud-radmir-radar__map{position:absolute!important;left:' +
                    px((width - w * sx) / 2) +
                    '!important;top:' +
                    px((height - h * sy) / 2) +
                    '!important;width:' +
                    px(w) +
                    '!important;height:' +
                    px(h) +
                    '!important;border:0!important;margin:0!important;padding:0!important;transform-origin:0 0!important;transform:scale3d(' +
                    sx +
                    ',' +
                    sy +
                    ',1)!important;}';
                css +=
                    r +
                    ' .hud-hassle-map{border-radius:0!important;}' +
                    r +
                    ' .map-mask{border-radius:0!important;overflow:visible!important;}';
                if (!radar.decorations)
                    css +=
                        r +
                        ' .hud-radmir-radar__radar-bats,' +
                        r +
                        ' .hud-radmir-radar__radar-border{display:none!important;}';
                if (radar.effect) {
                    const period = radar.effectPeriod + 's',
                        rainbow = [
                            '#ff4444',
                            '#ffcc33',
                            '#66ee55',
                            '#33ddff',
                            '#5566ff',
                            '#dd55ff',
                            '#ff4444'
                        ];
                    const frames =
                        radar.effect === 2
                            ? rainbow
                                  .map((c, i) => ((i * 100) / 6).toFixed(3) + '%{border-color:' + c + ';}')
                                  .join('')
                            : '0%,100%{border-color:' +
                              color(radar.effectFirst) +
                              ';}50%{border-color:' +
                              color(radar.effectSecond) +
                              ';}';
                    css +=
                        '@keyframes odRadarBorder{' +
                        frames +
                        '}@keyframes odRadarHue{0%{filter:hue-rotate(0deg);}100%{filter:hue-rotate(360deg);}}@keyframes odRadarTint{0%,100%{filter:sepia(1) saturate(3) hue-rotate(' +
                        hue(radar.effectFirst) +
                        'deg);}50%{filter:sepia(1) saturate(3) hue-rotate(' +
                        hue(radar.effectSecond) +
                        'deg);}}';
                    if (animatedBorder)
                        css += r + '{animation:odRadarBorder ' + period + ' linear infinite!important;}';
                    if (radar.effectTarget !== 0)
                        css +=
                            r +
                            ' .hud-hassle-map{animation:' +
                            (radar.effect === 2 ? 'odRadarHue' : 'odRadarTint') +
                            ' ' +
                            period +
                            ' linear infinite!important;}';
                }
            }
        } else clearMarker('data-od-radar');
        if (safety.enabled && state.safety) {
            const element = state.safety;
            if (!element.hasAttribute('data-od-safety')) element.setAttribute('data-od-safety', '');
            text(element, safety.text, labels);
            let left = safety.x,
                bottom = safety.bottom,
                width = 'auto';
            if (safety.followRadar && state.radar) {
                const rect = state.radar.getBoundingClientRect();
                left = rect.left;
                bottom = window.innerHeight - rect.top + safety.gap;
                width = px(rect.width);
            }
            const s = '#app [data-od-safety]';
            const anchor = viewportAnchor(element, left, bottom);
            css +=
                s +
                '{position:absolute!important;left:' +
                px(anchor.left) +
                '!important;bottom:' +
                px(anchor.bottom) +
                '!important;top:auto!important;right:auto!important;width:' +
                width +
                '!important;height:auto!important;min-height:' +
                px(safety.size * 1.3) +
                '!important;display:flex!important;align-items:center!important;justify-content:center!important;margin:0!important;padding:2px 4px!important;box-sizing:border-box!important;font-family:' +
                font(safety.font) +
                '!important;font-size:' +
                px(safety.size) +
                '!important;font-style:normal!important;font-weight:' +
                (safety.bold ? 700 : 400) +
                '!important;font-style:normal!important;line-height:normal!important;color:' +
                color(safety.color) +
                '!important;text-shadow:' +
                shadow(safety.outline, safety.outlineColor) +
                '!important;background-color:' +
                color(safety.background) +
                '!important;opacity:' +
                safety.opacity +
                '!important;pointer-events:none!important;}';
            css +=
                s +
                ' .hud-radmir-radar__safe-zone__bg{display:none!important;}' +
                s +
                ' svg{display:' +
                (safety.icon ? 'block' : 'none') +
                '!important;}' +
                s +
                ' svg [fill]{fill:' +
                color(safety.color) +
                '!important;}' +
                s +
                ' svg [stroke]{stroke:' +
                color(safety.color) +
                '!important;}';
        } else clearMarker('data-od-safety');
        restoreLabels(labels);
        if (chat.enabled && state.chat) {
            if (!state.chat.hasAttribute('data-od-chat')) state.chat.setAttribute('data-od-chat', '');
            if (window.App && window.App.$data) {
                const current = window.App.$data.chatPageSize;
                if (!state.chatRows) state.chatRows = { original: current, applied: null };
                else if (current !== state.chatRows.applied) state.chatRows.original = current;
                if (current !== chat.lines) window.App.$data.chatPageSize = chat.lines;
                state.chatRows.applied = chat.lines;
            }
            const c = '#app [data-od-chat]',
                selectors = [c + ' .chat-message-content', c + ' .chat-message-content *'];
            if (chat.input) selectors.push(c + ' input', c + ' textarea');
            css +=
                c +
                '{position:absolute!important;left:' +
                px(chat.x) +
                '!important;top:' +
                px(chat.y) +
                '!important;width:' +
                px(chat.width) +
                '!important;max-width:100000px!important;margin:0!important;opacity:' +
                chat.opacity +
                '!important;}';
            css +=
                c + ' .radmir-chat__container,' + c + ' .radmir-chat__messages' +
                '{width:100%!important;max-width:100%!important;min-width:0!important;box-sizing:border-box!important;}';
            if (chat.input)
                css += c + ' .radmir-chat-input{width:100%!important;max-width:100%!important;}';
            css +=
                selectors.join(',') +
                '{font-family:' +
                font(chat.font) +
                '!important;font-style:normal!important;font-weight:' +
                (chat.bold ? 700 : 400) +
                '!important;' +
                (chat.resize
                    ? 'font-size:' + px(chat.size) + '!important;line-height:' + px(chat.lineHeight) + '!important;'
                    : '') +
                'text-shadow:' +
                shadow(chat.outline, chat.outlineColor) +
                '!important;}';
            if (chat.resize) {
                css +=
                    c +
                    ' .radmir-chat__messages{height:' +
                    px(chat.lines * chat.lineHeight) +
                    '!important;max-height:' +
                    px(chat.lines * chat.lineHeight) +
                    '!important;}';
                css += c + ' .radmir-chat__controls{top:' + px(chat.lines * chat.lineHeight) + '!important;}';
            }
        } else {
            clearMarker('data-od-chat');
            restoreChat();
        }
        const message = cfg.gametext;
        if (message && message.enabled) {
            for (const node of document.querySelectorAll('.game-text'))
                if (!node.hasAttribute('data-od-gametext')) node.setAttribute('data-od-gametext', '');
            const g = '#app [data-od-gametext]',
                normal = g + ' .game-text--data .title',
                money = g + ' .right-type .game-text--data .title';
            css += g + ' .game-text--background{display:none!important;}';
            css +=
                normal +
                ',' +
                normal +
                ' *{font-family:' +
                font(message.font) +
                '!important;font-size:' +
                px(message.size) +
                '!important;font-style:normal!important;font-weight:' +
                (message.bold ? 700 : 400) +
                '!important;text-shadow:' +
                shadow(message.outline, message.outlineColor) +
                '!important;}';
            css += normal + '{transform:translate(' + px(message.x) + ',' + px(message.y) + ')!important;}';
            css +=
                money +
                ',' +
                money +
                ' *{font-family:' +
                font(message.moneyFont) +
                '!important;font-size:' +
                px(message.moneySize) +
                '!important;font-style:normal!important;font-weight:' +
                (message.bold ? 700 : 400) +
                '!important;text-shadow:' +
                shadow(message.moneyOutline, message.outlineColor) +
                '!important;}';
            css +=
                money +
                '{transform:translate(' +
                px(message.moneyX) +
                ',' +
                px(message.moneyY) +
                ') scale(1,0.65)!important;text-transform:lowercase!important;letter-spacing:0.03em!important;}';
            if (!message.preserveColors)
                css += normal + ',' + normal + ' *{color:' + color(message.color) + '!important;}';
        } else clearMarker('data-od-gametext');
        if (state.preview) {
            const p = '#__oldDialogsTextPreview';
            css +=
                p +
                '{position:absolute!important;left:0!important;top:0!important;width:100vw!important;height:100vh!important;display:flex!important;align-items:center!important;justify-content:center!important;z-index:1000!important;}' +
                p +
                '>span{display:flex!important;width:100%!important;height:100%!important;align-items:center!important;justify-content:center!important;}' +
                p +
                ' .game-text--content{position:relative!important;left:auto!important;right:auto!important;top:auto!important;bottom:auto!important;transform:none!important;}' +
                p +
                ' .title,' +
                p +
                ' .title *{color:rgba(255,255,255,1)!important;text-shadow:' +
                shadow(message ? Math.max(1, message.outline) : 4, [0, 0, 0, 1]) +
                '!important;}';
        }
        if (window.__oldDialogsSpeedometer) window.__oldDialogsSpeedometer.configure(cfg.speedometer);
        if (window.__oldDialogsPlayerHud) window.__oldDialogsPlayerHud.configure(cfg.playerHud);
        if (window.__oldDialogsLogo)
            window.__oldDialogsLogo.configure(cfg.logo, cfg.playerHud, cfg.nativeDpi);
        if (css !== state.css || (css && (!state.style || !state.style.parentNode))) {
            state.css = css;
            if (!css) {
                if (window.__oldDialogsStyles) window.__oldDialogsStyles.remove('__oldDialogsHudStyle');
                else if (state.style && state.style.parentNode) state.style.parentNode.removeChild(state.style);
                state.style = null;
            } else {
                state.style = window.__oldDialogsStyles.set('__oldDialogsHudStyle', css);
            }
        }
    }
    function schedule() {
        if (!state.timer && !state.disposed) state.timer = setTimeout(sync, 20);
    }
    const observer = new MutationObserver((records) => {
        for (const record of records)
            for (const node of record.addedNodes)
                if (
                    node.nodeType === 1 &&
                    ((typeof node.className === 'string' &&
                        /hud-radmir-radar|radmir-chat/.test(node.className)) ||
                        (typeof node.querySelector === 'function' &&
                            node.querySelector('.hud-radmir-radar,.radmir-chat')))
                ) {
                    schedule();
                    return;
                }
    });
    observer.observe(document.documentElement, { childList: true, subtree: true });
    window.addEventListener('resize', schedule);
    function restore() {
        clearMarker('data-od-radar');
        clearMarker('data-od-safety');
        clearMarker('data-od-chat');
        clearMarker('data-od-gametext');
        restoreChat();
        restoreLabels();
        if (window.__oldDialogsPlayerHud) window.__oldDialogsPlayerHud.configure(null);
        if (window.__oldDialogsLogo) window.__oldDialogsLogo.configure(null);
        if (window.__oldDialogsStyles) window.__oldDialogsStyles.remove('__oldDialogsHudStyle');
        else if (state.style && state.style.parentNode) state.style.parentNode.removeChild(state.style);
        state.style = null;
        state.css = '';
    }
    const lease = setInterval(() => {
        if (Date.now() - state.seen > 15000) {
            state.expired = true;
            state.config = null;
            restore();
        }
    }, 1000);
    const api = {
        version: 4,
        layoutChanged() {
            sync();
            return {};
        },
        configure(config) {
            state.expired = false;
            state.seen = Date.now();
            state.config = config;
            sync();
            requestAnimationFrame(schedule);
            return api.status();
        },
        previewMessage(kind) {
            if (state.preview) state.preview.remove();
            clearTimeout(state.previewTimer);
            const original = document.querySelector('.game-text');
            const root = document.createElement('div');
            root.className = 'game-text';
            root.id = '__oldDialogsTextPreview';
            root.innerHTML =
                kind === 1
                    ? '<span><div class="game-text--content right-type"><div class="game-text--data"><div class="title"><div><div class="row"><span class="g">+20 000 ₽</span></div></div></div></div></div></span>'
                    : '<span><div class="game-text--content bottom-type"><div class="game-text--data"><div class="title"><div><div class="row"><span class="w">Двигатель</span><span class="g">заведён</span></div></div></div></div></div></span>';
            if (original) {
                const attrs = Array.from(original.attributes).filter((a) => a.name.indexOf('data-v-') === 0);
                for (const node of [root].concat(Array.from(root.querySelectorAll('*'))))
                    for (const attr of attrs) node.setAttribute(attr.name, String(attr.value));
            }
            document.getElementById('app').appendChild(root);
            state.preview = root;
            sync();
            state.previewTimer = setTimeout(() => {
                root.remove();
                if (state.preview === root) state.preview = null;
            }, 5000);
            return {};
        },
        status() {
            state.seen = Date.now();
            sync();
            return {
                expired: state.expired,
                radar: !!state.radar,
                safety: !!state.safety,
                chat: !!state.chat,
                radarCustomized: !!document.querySelector('[data-od-radar]'),
                safetyCustomized: !!document.querySelector('[data-od-safety]'),
                chatCustomized: !!document.querySelector('[data-od-chat]')
            };
        },
        shutdown() {
            state.disposed = true;
            clearTimeout(state.previewTimer);
            if (state.preview) state.preview.remove();
            if (window.__oldDialogsSpeedometer) window.__oldDialogsSpeedometer.shutdown();
            if (window.__oldDialogsPlayerHud) window.__oldDialogsPlayerHud.shutdown();
            if (window.__oldDialogsLogo) window.__oldDialogsLogo.shutdown();
            clearInterval(lease);
            clearTimeout(state.timer);
            observer.disconnect();
            window.removeEventListener('resize', schedule);
            restore();
            delete window.__oldDialogsHud;
            return true;
        }
    };
    window.__oldDialogsHud = api;
    return api;
})();
