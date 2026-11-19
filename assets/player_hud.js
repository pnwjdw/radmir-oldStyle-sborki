(function () {
    if (window.__oldDialogsPlayerHud) window.__oldDialogsPlayerHud.shutdown();
    const id = '__oldDialogsPlayerHudStyle';
    const css = '#app .hud-radmir-info,#app .hud-hassle-info{visibility:hidden!important;pointer-events:none!important;}';
    let style = null,
        current = 0;
    const number = (value) => {
        const result = Number(value);
        return Number.isFinite(result) ? result : 0;
    };
    function clearStyle() {
        if (window.__oldDialogsStyles) window.__oldDialogsStyles.remove(id);
        else if (style && style.parentNode) style.parentNode.removeChild(style);
        style = null;
    }
    function updateStyle(visible) {
        if (!current || !visible) {
            clearStyle();
            return;
        }
        if (window.__oldDialogsStyles) {
            style = window.__oldDialogsStyles.set(id, css);
        } else if (!style || !style.parentNode) {
            style = document.createElement('style');
            style.id = id;
            style.textContent = css;
            document.head.appendChild(style);
        }
    }
    function read() {
        const hud = window.getInterface && window.getInterface('Hud'),
            info = hud && hud.info;
        if (!info) {
            updateStyle(false);
            return { visible: false };
        }
        const value = {
            visible: !!info.show && !hud.hudIsManuallyHidden,
            bars: !!info.showBars,
            inWater: !!hud.isPlayerSwiming,
            weapon: Math.max(0, Math.trunc(number(info.weapon))),
            ammo: Math.max(0, Math.trunc(number(info.ammoInClip))),
            totalAmmo: Math.max(0, Math.trunc(number(info.totalAmmo))),
            money: String(Math.trunc(number(info.money))),
            health: number(info.health),
            armour: number(info.armour),
            hunger: number(info.hunger),
            air: number(info.breath),
            wanted: Math.max(0, Math.min(6, Math.trunc(number(info.wanted)))),
            server: Math.max(0, Math.trunc(number(hud.server)))
        };
        updateStyle(value.visible);
        return value;
    }
    const api = {
        version: 2,
        configure(config) {
            current = (config && config.style) || 0;
            read();
        },
        read: read,
        status() {
            return { version: 2, style: current };
        },
        shutdown() {
            clearStyle();
            delete window.__oldDialogsPlayerHud;
        }
    };
    window.__oldDialogsPlayerHud = api;
    return api;
})();
