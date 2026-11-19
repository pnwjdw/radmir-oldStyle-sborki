(function () {
    if (window.__oldDialogsSpeedometer) window.__oldDialogsSpeedometer.shutdown();
    const id = '__oldDialogsSpeedometerStyle';
    const css = '#app .hud-radmir-speedometer{visibility:hidden!important;pointer-events:none!important;}';
    let style = null,
        current = 0;
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
            car = hud && hud.speedometer,
            model = window.Speedometer || {};
        if (!car) {
            updateStyle(false);
            return { visible: false };
        }
        const p = car.params || {};
        const directSpeed = Number(car.speed);
        const speed = Number.isFinite(directSpeed)
            ? directSpeed
            : Number(
                  window.hudSpeedometer
                      ? window.hudSpeedometer.formatSpeed(model.speed || 0, 0)
                      : model.speed
              ) || 0;
        const value = {
            visible: !!car.show && !hud.hudIsManuallyHidden,
            speed: Math.max(0, speed),
            fuel: Math.max(0, Number(car.fuel) || 0),
            maxFuel: Math.max(1, Number(car.maxFuel) || 100),
            mileage: String(car.mileage || 0),
            engine: !!p.temperature,
            lights: !!p.lights,
            belt: !!p.rem,
            locked: !!p.doors,
            key: !!p.key,
            alarm: !!p.alarm,
            left: !!(p.turns && p.turns.left),
            right: !!(p.turns && p.turns.right)
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
            return { version: 2, style: current, visible: !!current && read().visible };
        },
        shutdown() {
            clearStyle();
            delete window.__oldDialogsSpeedometer;
        }
    };
    window.__oldDialogsSpeedometer = api;
    return api;
})();
