(function () {
    if (window.__oldDialogsLogo) window.__oldDialogsLogo.shutdown();
    let config = null,
        hud = null,
        dpi = 1,
        template = '',
        style = null,
        root = null,
        title = null,
        subtitle = null,
        glyph = null,
        number = null,
        lastRules = '',
        lastPosition = '';
    const palette = [
        '#ffae00',
        '#9548d4',
        '#35c6da',
        '#47af71',
        '#f15352',
        '#cf2858',
        '#b7aa58',
        '#c6d6d6',
        '#ff9600',
        '#a6d513',
        '#d953da',
        '#5ed3bb',
        '#c0c84b',
        '#f77226',
        '#5842f6',
        '#3c91e7',
        '#f3478d',
        '#1ab7a6',
        '#dca326',
        '#8061e9',
        '#db364c'
    ];
    const rgba = (c, alpha) =>
        'rgba(' +
        c
            .slice(0, 3)
            .map((n) => Math.round(n * 255))
            .join(',') +
        ',' +
        (alpha === undefined ? c[3] : alpha) +
        ')';
    function serverColor(server) {
        if (server > 0 && server <= palette.length) return palette[server - 1];
        const hue = ((server || 1) * 0.38196601125) % 1,
            part = hue * 6,
            index = Math.floor(part),
            fraction = part - index,
            p = 0.22,
            q = 1 - 0.78 * fraction,
            t = 0.22 + 0.78 * fraction;
        const rgb = [
            [1, t, p],
            [q, 1, p],
            [p, 1, t],
            [p, q, 1],
            [t, p, 1],
            [1, p, q]
        ][index % 6];
        return rgba(rgb.concat([1]));
    }
    function clear() {
        if (root) root.remove();
        window.__oldDialogsStyles.remove('__oldDialogsLogoStyle');
        root = style = title = subtitle = glyph = number = null;
        lastRules = lastPosition = '';
    }
    function node(name, parent) {
        const result = document.createElement('div');
        result.className = name;
        parent.appendChild(result);
        return result;
    }
    function refresh() {
        if (!config || !config.style || config.customActive) {
            clear();
            return;
        }
        const native = window.getInterface && window.getInterface('Hud');
        if (!root) {
            root = document.createElement('div');
            root.id = '__oldDialogsLogo';
            const mark = node('odl-mark', root);
            glyph = node('odl-glyph', mark);
            const copy = node('odl-copy', root);
            title = node('odl-title', copy);
            subtitle = node('odl-subtitle', copy);
            number = node('odl-number', root);
            (document.getElementById('app') || document.body).appendChild(root);
            style = null;
        }
        const raw = config.server || (native && native.server) || 0,
            server = Number.isFinite(Number(raw)) ? Math.max(0, Math.trunc(Number(raw))) : 0,
            label = server ? String(server) : '?';
        const accent = config.autoColor ? serverColor(server) : rgba(config.accent),
            glow = config.autoColor ? accent : rgba(config.accent, 0.4);
        const titleText = String(config.title || ''),
            subtitleText = String(config.subtitle || ''),
            emblemText = String(config.emblem || '');
        if (title.textContent !== titleText) title.textContent = titleText;
        if (subtitle.textContent !== subtitleText) subtitle.textContent = subtitleText;
        if (glyph.textContent !== emblemText) glyph.textContent = emblemText;
        if (number.textContent !== label) number.textContent = label;
        const sizes = {
            FONT: 'OldDialogsLocal11_' + config.font + ',Arial,sans-serif',
            TEXT: rgba(config.color),
            BACKGROUND: rgba(config.background),
            EMBLEM: rgba(config.emblemColor),
            ACCENT: accent,
            GLOW: glow,
            ROUND: config.rounding,
            TITLE_SIZE: Math.max(7, Math.min(16, (120 / Math.max(1, titleText.length)) * 1.3)),
            SUB_SIZE: Math.max(5, Math.min(8, 130 / Math.max(1, subtitleText.length))),
            TRACKING: titleText.length > 10 ? 1 : 3,
            NUMBER_SIZE: Math.max(9, Math.min(30, 52 / Math.max(1, label.length))),
            EMBLEM_SIZE: Math.max(8, Math.min(32, 44 / Math.max(1, emblemText.length))),
            FIRST: rgba(config.effectFirst),
            SECOND: rgba(config.effectSecond),
            PERIOD: config.effectPeriod
        };
        const rules = template.replace(/@([A-Z_]+)\s*@/g, (match, key) =>
            String(sizes[key] === undefined ? '' : sizes[key])
        );
        if (rules !== lastRules) {
            style = window.__oldDialogsStyles.set('__oldDialogsLogoStyle', rules);
            lastRules = rules;
        }
        const type =
                ['', 'odl-classic', 'odl-neon', 'odl-ribbon', 'odl-minimal'][config.style] || 'odl-classic',
            classes =
                type + (config.effect === 2 ? ' odl-rainbow' : config.effect === 1 ? ' odl-colors' : '');
        if (root.className !== classes) root.className = classes;
        const attached = !!config.followHud && hud && hud.style,
            right = attached ? hud.right : config.right,
            top = attached
                ? Math.max(hud.top, config.top + config.height * config.scale + config.gap) -
                  config.height * config.scale -
                  config.gap
                : config.top;
        const width = config.width * config.scale * dpi,
            left = Math.max(0, window.innerWidth - right * dpi - width),
            visible = !!(native && native.info && native.info.show && !native.hudIsManuallyHidden);
        const position =
            'left:' +
            left +
            'px;top:' +
            top * dpi +
            'px;transform:scale(' +
            (config.width / 252) * config.scale * dpi +
            ',' +
            (config.height / 64) * config.scale * dpi +
            ');opacity:' +
            config.opacity +
            ';visibility:' +
            (visible ? 'visible' : 'hidden') +
            ';';
        if (position !== lastPosition) {
            root.style.cssText = position;
            lastPosition = position;
        }
    }
    const timer = setInterval(refresh, 250);
    const api = {
        configure(value, player, scale, css) {
            config = value;
            hud = player;
            dpi = scale || 1;
            if (css !== undefined) template = css;
            refresh();
            return api.status();
        },
        status() {
            const native = window.getInterface && window.getInterface('Hud');
            return {
                style: (config && config.style) || 0,
                server: (config && config.server) || (native && native.server) || 0,
                visible: !!(root && root.isConnected && root.style.visibility === 'visible' && Number(root.style.opacity) > 0),
                palette: palette.length
            };
        },
        shutdown() {
            clearInterval(timer);
            clear();
            delete window.__oldDialogsLogo;
        }
    };
    window.__oldDialogsLogo = api;
    return api;
})();
