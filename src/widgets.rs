use crate::settings::Config;
use base64::Engine;
use egui::{
    Align2, Color32, FontFamily, FontId, Id, LayerId, Order, Painter, Pos2, Rect, Shape, Stroke,
    TextureHandle, Vec2, pos2, vec2,
};
use image::{AnimationDecoder, ImageDecoder};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufReader, Cursor};
use std::time::Instant;

fn value<'a>(data: &'a Value, key: &str) -> &'a Value {
    if let Some(value) = data.get(key) {
        return value;
    }
    let mut alternative = String::new();
    for c in key.chars() {
        if c.is_uppercase() {
            alternative.push('_');
            alternative.extend(c.to_lowercase());
        } else {
            alternative.push(c);
        }
    }
    data.get(&alternative).unwrap_or(&Value::Null)
}

fn n(data: &Value, key: &str) -> f32 {
    value(data, key)
        .as_f64()
        .or_else(|| value(data, key).as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0) as f32
}

fn b(data: &Value, key: &str) -> bool {
    value(data, key).as_bool().unwrap_or(false)
}
fn s<'a>(data: &'a Value, key: &str) -> &'a str {
    value(data, key).as_str().unwrap_or("")
}

fn rgba(data: &Value, key: &str) -> [f32; 4] {
    let v = value(data, key);
    [
        v[0].as_f64().unwrap_or(1.0) as f32,
        v[1].as_f64().unwrap_or(1.0) as f32,
        v[2].as_f64().unwrap_or(1.0) as f32,
        v[3].as_f64().unwrap_or(1.0) as f32,
    ]
}

fn ink(c: [f32; 4], opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[3].clamp(0.0, 1.0) * opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn animated(mode: i32, first: [f32; 4], second: [f32; 4], phase: f32) -> [f32; 4] {
    if mode == 2 {
        let hsv = egui::ecolor::Hsva::new(phase.rem_euclid(1.0), 0.85, 1.0, first[3]);
        return hsv.to_rgba_unmultiplied();
    }
    let amount = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
    std::array::from_fn(|i| first[i] + (second[i] - first[i]) * amount)
}

#[derive(Clone)]
struct Picture {
    id: egui::TextureId,
    uv: Rect,
    aspect: f32,
}

struct PictureFrame {
    texture: TextureHandle,
    duration_ms: u64,
}

struct LoadedPicture {
    source: String,
    frames: Vec<PictureFrame>,
    uv: Rect,
    aspect: f32,
    started: Instant,
}

impl LoadedPicture {
    fn current(&self) -> Option<Picture> {
        let total: u64 = self.frames.iter().map(|f| f.duration_ms).sum();
        let mut time = if total > 0 {
            self.started.elapsed().as_millis() as u64 % total
        } else {
            0
        };
        let frame = self
            .frames
            .iter()
            .find(|f| {
                if time < f.duration_ms {
                    true
                } else {
                    time -= f.duration_ms;
                    false
                }
            })
            .or_else(|| self.frames.first())?;
        Some(Picture {
            id: frame.texture.id(),
            uv: self.uv,
            aspect: self.aspect,
        })
    }
}

struct Canvas<'a> {
    painter: Painter,
    origin: Pos2,
    scale: f32,
    opacity: f32,
    fonts: &'a [String],
    font: usize,
}

impl<'a> Canvas<'a> {
    fn point(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.scale
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.point(x, y), vec2(w.max(0.0), h.max(0.0)) * self.scale)
    }
    fn fill(&self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], rounding: f32) {
        self.painter.rect_filled(
            self.rect(x, y, w, h),
            rounding.max(0.0) * self.scale,
            ink(color, self.opacity),
        );
    }
    fn stroke(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        rounding: f32,
        thickness: f32,
    ) {
        if thickness > 0.0 {
            self.painter.rect_stroke(
                self.rect(x, y, w, h),
                rounding.max(0.0) * self.scale,
                Stroke::new(thickness * self.scale, ink(color, self.opacity)),
                egui::StrokeKind::Middle,
            );
        }
    }
    fn line(&self, a: (f32, f32), z: (f32, f32), color: [f32; 4], thickness: f32) {
        self.painter.line_segment(
            [self.point(a.0, a.1), self.point(z.0, z.1)],
            Stroke::new(thickness * self.scale, ink(color, self.opacity)),
        );
    }
    fn circle(&self, x: f32, y: f32, radius: f32, color: [f32; 4]) {
        self.painter.circle_filled(
            self.point(x, y),
            radius * self.scale,
            ink(color, self.opacity),
        );
    }
    fn arc(
        &self,
        cx: f32,
        cy: f32,
        radius: f32,
        start: f32,
        end: f32,
        color: [f32; 4],
        thickness: f32,
    ) {
        let steps = ((end - start).abs() / 4.0).ceil().max(8.0) as usize;
        let points = (0..=steps)
            .map(|i| {
                let angle = (start + (end - start) * i as f32 / steps as f32).to_radians();
                self.point(cx + angle.cos() * radius, cy + angle.sin() * radius)
            })
            .collect();
        self.painter.add(Shape::line(
            points,
            Stroke::new(thickness * self.scale, ink(color, self.opacity)),
        ));
    }
    fn font_id(&self, index: usize, size: f32, bold: bool) -> FontId {
        let family = self.fonts.get(index).map(|name| {
            if bold {
                format!("{name} Bold")
            } else {
                name.clone()
            }
        });
        let family = family
            .map(|name| FontFamily::Name(name.into()))
            .filter(|family| {
                self.painter
                    .ctx()
                    .fonts(|fonts| fonts.families().contains(family))
            })
            .unwrap_or(FontFamily::Proportional);
        FontId::new((size * self.scale).max(1.0), family)
    }
    fn measure(&self, text: &str, size: f32, font: usize, bold: bool) -> Vec2 {
        self.painter
            .layout_no_wrap(
                text.to_owned(),
                self.font_id(font, size, bold),
                Color32::WHITE,
            )
            .size()
            / self.scale
    }
    fn text(&self, x: f32, y: f32, size: f32, text: &str, color: [f32; 4], centered: bool) {
        self.outlined(
            x, y, size, self.font, text, color, [0.0; 4], 0.0, centered, true,
        );
    }
    #[allow(clippy::too_many_arguments)]
    fn outlined(
        &self,
        x: f32,
        y: f32,
        size: f32,
        font: usize,
        text: &str,
        color: [f32; 4],
        outline: [f32; 4],
        thickness: f32,
        centered: bool,
        bold: bool,
    ) {
        let align = if centered {
            Align2::CENTER_TOP
        } else {
            Align2::LEFT_TOP
        };
        let pos = self.point(x, y);
        let font = self.font_id(font, size, bold);
        if thickness > 0.0 && outline[3] > 0.0 {
            let steps = (thickness * 8.0).ceil().max(8.0) as usize;
            for step in 0..steps {
                let angle = step as f32 * std::f32::consts::TAU / steps as f32;
                self.painter.text(
                    pos + vec2(angle.cos(), angle.sin()) * thickness * self.scale,
                    align,
                    text,
                    font.clone(),
                    ink(outline, self.opacity),
                );
            }
        }
        self.painter
            .text(pos, align, text, font, ink(color, self.opacity));
    }
    fn image(&self, picture: &Picture, x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) {
        self.painter.image(
            picture.id,
            self.rect(x, y, width, height),
            picture.uv,
            ink(color, self.opacity),
        );
    }
    fn image_round(
        &self,
        picture: &Picture,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [f32; 4],
        rounding: f32,
    ) {
        self.painter.add(
            egui::epaint::RectShape::filled(
                self.rect(x, y, width, height),
                rounding.max(0.0) * self.scale,
                ink(color, self.opacity),
            )
            .with_texture(picture.id, picture.uv),
        );
    }
    fn rotated(&self, picture: &Picture, cx: f32, cy: f32, side: f32, angle: f32) {
        let angle = angle.to_radians();
        let (sin, cos) = angle.sin_cos();
        let half = side * 0.5;
        let corners = [(-half, -half), (half, -half), (half, half), (-half, half)];
        let uvs = [
            picture.uv.left_top(),
            picture.uv.right_top(),
            picture.uv.right_bottom(),
            picture.uv.left_bottom(),
        ];
        let mut mesh = egui::Mesh::with_texture(picture.id);
        for (index, (x, y)) in corners.into_iter().enumerate() {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: self.point(cx + x * cos - y * sin, cy + x * sin + y * cos),
                uv: uvs[index],
                color: ink([1.0; 4], self.opacity),
            });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        self.painter.add(mesh);
    }
    fn child(&self, origin: Pos2, scale: f32) -> Canvas<'a> {
        Canvas {
            painter: self.painter.clone(),
            origin,
            scale,
            opacity: self.opacity,
            fonts: self.fonts,
            font: self.font,
        }
    }
}

pub struct Widgets {
    pictures: HashMap<String, LoadedPicture>,
    failed_pictures: HashMap<String, String>,
    weapon_sources: Value,
    speedometer_sources: Value,
    pub elements: Vec<Value>,
    pub custom_logo_active: bool,
    display_speed: f32,
}

impl Default for Widgets {
    fn default() -> Self {
        Self::new()
    }
}

impl Widgets {
    pub fn new() -> Self {
        Self {
            pictures: HashMap::new(),
            failed_pictures: HashMap::new(),
            weapon_sources: serde_json::from_str(crate::assets::PLAYER_HUD_IMAGES)
                .unwrap_or(Value::Null),
            speedometer_sources: serde_json::from_str(crate::assets::SPEEDOMETER_IMAGES)
                .unwrap_or(Value::Null),
            elements: Vec::new(),
            custom_logo_active: false,
            display_speed: 0.0,
        }
    }

    fn picture(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        source: &str,
        crop: bool,
    ) -> Option<Picture> {
        if self
            .pictures
            .get(id)
            .is_some_and(|loaded| loaded.source == source)
        {
            return self.pictures.get(id)?.current();
        }
        if self
            .failed_pictures
            .get(id)
            .is_some_and(|failed| failed == source)
        {
            return None;
        }
        let result = self.load_picture(ctx, id, source, crop);
        match result {
            Ok(loaded) => {
                self.failed_pictures.remove(id);
                self.pictures.insert(id.into(), loaded);
                self.pictures.get(id)?.current()
            }
            Err(_) => {
                self.failed_pictures.insert(id.into(), source.into());
                self.pictures.remove(id);
                None
            }
        }
    }

    fn load_picture(
        &self,
        ctx: &egui::Context,
        id: &str,
        source: &str,
        crop: bool,
    ) -> Result<LoadedPicture, String> {
        let bytes = if source.starts_with("data:") {
            let (_, payload) = source.split_once(',').ok_or("Invalid image data URI")?;
            base64::engine::general_purpose::STANDARD
                .decode(
                    payload
                        .chars()
                        .filter(|c| !c.is_ascii_whitespace())
                        .collect::<String>(),
                )
                .map_err(|e| e.to_string())?
        } else if source.starts_with("file:") || std::path::Path::new(source).is_file() {
            let path = source
                .strip_prefix("file:///")
                .or_else(|| source.strip_prefix("file://"))
                .unwrap_or(source);
            let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
            if metadata.len() > 33_554_432 {
                return Err("Image exceeds 32 MiB".into());
            }
            std::fs::read(path).map_err(|e| e.to_string())?
        } else {
            base64::engine::general_purpose::STANDARD
                .decode(
                    source
                        .chars()
                        .filter(|c| !c.is_ascii_whitespace())
                        .collect::<String>(),
                )
                .map_err(|e| e.to_string())?
        };
        if bytes.len() > 33_554_432 {
            return Err("Image exceeds 32 MiB".into());
        }
        let mut decoded = Vec::new();
        let mut decoded_bytes = 0usize;
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(Cursor::new(&bytes)))
                .map_err(|e| e.to_string())?;
            let (width, height) = decoder.dimensions();
            if width == 0 || height == 0 || width > 2048 || height > 2048 {
                return Err("Invalid image dimensions".into());
            }
            for (index, frame) in decoder.into_frames().enumerate() {
                if index >= 240 {
                    return Err("Too many GIF frames".into());
                }
                let frame = frame.map_err(|e| e.to_string())?;
                let (num, den) = frame.delay().numer_denom_ms();
                let delay = if den == 0 || num == 0 {
                    100
                } else {
                    (num as u64 / den as u64).clamp(20, 10_000)
                };
                let pixels = frame.into_buffer();
                decoded_bytes = decoded_bytes.saturating_add(pixels.as_raw().len());
                if decoded_bytes > 100_663_296 {
                    return Err("GIF exceeds 96 MiB decoded".into());
                }
                decoded.push((pixels, delay));
            }
        } else {
            let (width, height) = image::ImageReader::new(Cursor::new(&bytes))
                .with_guessed_format()
                .map_err(|e| e.to_string())?
                .into_dimensions()
                .map_err(|e| e.to_string())?;
            if width == 0 || height == 0 || width > 2048 || height > 2048 {
                return Err("Invalid image dimensions".into());
            }
            decoded.push((
                image::load_from_memory(&bytes)
                    .map_err(|e| e.to_string())?
                    .to_rgba8(),
                100,
            ));
        }
        let first = decoded.first().ok_or("Image contains no frames")?;
        let (width, height) = first.0.dimensions();
        if width == 0
            || height == 0
            || width > 2048
            || height > 2048
            || decoded
                .iter()
                .map(|(image, _)| image.as_raw().len())
                .sum::<usize>()
                > 100_663_296
        {
            return Err("Invalid image dimensions".into());
        }
        let mut bounds = [width, height, 0, 0];
        if crop {
            for (image, _) in &decoded {
                for (x, y, pixel) in image.enumerate_pixels() {
                    if pixel[3] > 16 {
                        bounds[0] = bounds[0].min(x);
                        bounds[1] = bounds[1].min(y);
                        bounds[2] = bounds[2].max(x);
                        bounds[3] = bounds[3].max(y);
                    }
                }
            }
        }
        let uv = if crop && bounds[0] <= bounds[2] && bounds[1] <= bounds[3] {
            Rect::from_min_max(
                pos2(
                    bounds[0].saturating_sub(2) as f32 / width as f32,
                    bounds[1].saturating_sub(2) as f32 / height as f32,
                ),
                pos2(
                    (bounds[2] + 3).min(width) as f32 / width as f32,
                    (bounds[3] + 3).min(height) as f32 / height as f32,
                ),
            )
        } else {
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))
        };
        let aspect = width as f32 * uv.width() / (height as f32 * uv.height()).max(1.0);
        let frames = decoded
            .into_iter()
            .enumerate()
            .map(|(index, (image, duration_ms))| {
                let size = [image.width() as usize, image.height() as usize];
                let texture = ctx.load_texture(
                    format!("oldDialogs-{id}-{index}"),
                    egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                    egui::TextureOptions::LINEAR,
                );
                PictureFrame {
                    texture,
                    duration_ms,
                }
            })
            .collect();
        Ok(LoadedPicture {
            source: source.into(),
            frames,
            uv,
            aspect,
            started: Instant::now(),
        })
    }

    fn bounds(
        &mut self,
        data: &Value,
        id: &str,
        label: &str,
        origin: Pos2,
        size: Vec2,
        geometry_scale: f32,
    ) -> (Pos2, f32) {
        let layout = &data["layouts"][id];
        let parent_scale = if id.starts_with("hud.") {
            data["layouts"]["hud"]["scale"]
                .as_f64()
                .unwrap_or(1.0)
                .clamp(0.25, 4.0) as f32
        } else {
            1.0
        };
        let mut offset = vec2(n(layout, "x"), n(layout, "y"));
        if id == "logo"
            && b(&data["widgets"]["logo"], "followHud")
            && n(&data["widgets"]["playerHud"], "style") > 0.0
        {
            offset += vec2(
                n(&data["layouts"]["hud"], "x"),
                n(&data["layouts"]["hud"], "y"),
            );
        }
        let scale = layout["scale"].as_f64().unwrap_or(1.0).clamp(0.25, 4.0) as f32;
        let origin = origin + offset * parent_scale;
        self.elements.push(json!({"id":id,"label":label,"x":origin.x,"y":origin.y,"width":size.x*geometry_scale*scale,"height":size.y*geometry_scale*scale,"parent_scale":parent_scale}));
        (origin, geometry_scale * scale)
    }

    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        vehicle: &Value,
        player: &Value,
        menu_open: bool,
    ) -> bool {
        let weapon_id = (n(player, "weapon") as u32).to_string();
        let current_weapon_key = format!("weapon-custom-{weapon_id}");
        let logo_selected = n(&config.data["widgets"]["logo"], "style") > 0.0
            && n(&config.data["widgets"]["logo"], "customMode") > 0.0;
        let retain = |id: &str| {
            if id.starts_with("weapon-custom-") {
                return id == current_weapon_key;
            }
            if let Some(index) = id.strip_prefix("status-") {
                return config.data["statusIcons"].get(index).is_some();
            }
            if id == "custom-logo" {
                return logo_selected;
            }
            true
        };
        self.pictures.retain(|id, _| retain(id));
        self.failed_pictures.retain(|id, _| retain(id));
        self.elements.clear();
        self.custom_logo_active = false;
        let screen = ctx.screen_rect();
        let layer = if menu_open {
            Order::Middle
        } else {
            Order::Foreground
        };
        let painter = ctx.layer_painter(LayerId::new(layer, Id::new("oldDialogs-widgets")));
        let root = Canvas {
            painter,
            origin: screen.min,
            scale: 1.0,
            opacity: 1.0,
            fonts: &config.font_names,
            font: 0,
        };
        let now = crate::bridge::ticks();
        if b(vehicle, "visible")
            && now.saturating_sub(value(vehicle, "received").as_u64().unwrap_or(now)) <= 2000
        {
            self.speedometer(&root, &config.data, vehicle, screen);
        }
        if b(player, "visible")
            && now.saturating_sub(value(player, "received").as_u64().unwrap_or(now)) <= 2000
        {
            self.hud(&root, &config.data, player, screen);
            self.logo(&root, &config.data, player, screen);
        }
        self.clock(&root, &config.data, screen);
        if self
            .pictures
            .values()
            .any(|picture| picture.frames.len() > 1)
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        false
    }

    fn clock(&mut self, root: &Canvas<'_>, data: &Value, screen: Rect) {
        let cfg = &data["widgets"]["clock"];
        if !b(cfg, "enabled") {
            return;
        }
        #[repr(C)]
        #[derive(Default)]
        struct SystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            milliseconds: u16,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLocalTime(time: *mut SystemTime);
        }
        let mut time = SystemTime::default();
        unsafe {
            GetLocalTime(&mut time);
        }
        let hour = if b(cfg, "format24") {
            time.hour
        } else {
            let h = time.hour % 12;
            if h == 0 { 12 } else { h }
        };
        let mut text = format!("{:02}:{:02}", hour, time.minute);
        if b(cfg, "seconds") || b(cfg, "milliseconds") {
            text.push_str(&format!(":{:02}", time.second));
        }
        if b(cfg, "milliseconds") {
            text.push_str(&format!(".{:03}", time.milliseconds));
        }
        if !b(cfg, "format24") {
            text.push_str(if time.hour < 12 { " AM" } else { " PM" });
        }
        let font = n(cfg, "font") as usize;
        let extent = root.measure(&text, n(cfg, "size"), font, b(cfg, "bold"))
            + vec2(n(cfg, "paddingX"), n(cfg, "paddingY")) * 2.0;
        let anchor = n(cfg, "anchor") as u32;
        let origin = pos2(
            screen.left()
                + ((screen.width() - extent.x) * (anchor % 3) as f32 * 0.5 + n(cfg, "x"))
                    .clamp(0.0, (screen.width() - extent.x).max(0.0)),
            screen.top()
                + ((screen.height() - extent.y) * (anchor / 3) as f32 * 0.5 + n(cfg, "y"))
                    .clamp(0.0, (screen.height() - extent.y).max(0.0)),
        );
        let (origin, scale) = self.bounds(data, "clock", "Часы", origin, extent, 1.0);
        let c = root.child(origin, scale);
        c.fill(
            0.0,
            0.0,
            extent.x,
            extent.y,
            rgba(cfg, "background"),
            n(cfg, "rounding"),
        );
        c.stroke(
            0.0,
            0.0,
            extent.x,
            extent.y,
            rgba(cfg, "borderColor"),
            n(cfg, "rounding"),
            n(cfg, "border"),
        );
        if b(cfg, "shadow") {
            c.outlined(
                n(cfg, "paddingX") + n(cfg, "shadowX"),
                n(cfg, "paddingY") + n(cfg, "shadowY"),
                n(cfg, "size"),
                font,
                &text,
                rgba(cfg, "shadowColor"),
                [0.0; 4],
                0.0,
                false,
                b(cfg, "bold"),
            );
        }
        c.outlined(
            n(cfg, "paddingX"),
            n(cfg, "paddingY"),
            n(cfg, "size"),
            font,
            &text,
            rgba(cfg, "color"),
            rgba(cfg, "outlineColor"),
            n(cfg, "outline"),
            false,
            b(cfg, "bold"),
        );
    }

    fn hud(&mut self, root: &Canvas<'_>, data: &Value, player: &Value, screen: Rect) {
        let cfg = &data["widgets"]["playerHud"];
        let style = n(cfg, "style") as i32;
        if style == 0 {
            return;
        }
        let logo = &data["widgets"]["logo"];
        let side = if b(cfg, "weapon") {
            n(cfg, "weaponSize")
        } else {
            0.0
        };
        let gap = if side > 0.0 { n(cfg, "gap") } else { 0.0 };
        let width = n(cfg, "width").max(side + gap + 80.0);
        let scale = n(cfg, "scale");
        let top = if n(logo, "style") > 0.0 && b(logo, "followHud") {
            n(cfg, "top")
                .max(n(logo, "top") + n(logo, "height") * n(logo, "scale") + n(logo, "gap"))
        } else {
            n(cfg, "top")
        };
        let origin = pos2(
            (screen.right() - n(cfg, "right") - width * scale).max(screen.left()),
            screen.top() + top,
        );
        let hud_index = self.elements.len();
        let (origin, scale) = self.bounds(
            data,
            "hud",
            "HUD целиком",
            origin,
            vec2(width, side + 100.0),
            scale,
        );
        let mut c = root.child(origin, scale);
        c.opacity = n(cfg, "opacity");
        let names = ["health", "armour", "hunger", "air"];
        let labels = ["Здоровье", "Броня", "Голод", "Воздух"];
        let mut bars = Vec::new();
        if b(player, "bars") {
            for row in ["row0", "row1", "row2", "row3"] {
                let id = (n(cfg, row) as usize).min(3);
                if b(cfg, names[id]) && (id != 3 || b(player, "inWater")) {
                    bars.push(id);
                }
            }
        }
        let ammo = b(cfg, "weapon")
            && b(cfg, "ammo")
            && n(player, "weapon") >= 16.0
            && (n(player, "weapon") <= 43.0
                || n(player, "ammo") > 0.0
                || n(player, "totalAmmo") > 0.0);
        let weapon_x = if style == 2 { width - side } else { 0.0 };
        let weapon_y = if style == 2 { 3.0 } else { 0.0 };
        if b(cfg, "weapon") {
            self.weapon(
                &c,
                data,
                cfg,
                player,
                weapon_x,
                weapon_y,
                if ammo {
                    format!(
                        "{} - {}",
                        n(player, "ammo") as u32,
                        n(player, "totalAmmo") as u32
                    )
                } else {
                    String::new()
                },
            );
        }
        let step = n(cfg, "barHeight") + n(cfg, "barGap");
        let column = width - side - gap;
        let mut bottom;
        if style == 2 {
            let mut y = self.money(&c, data, cfg, player, 0.0, 0.0, column);
            if y > 0.0 {
                y += n(cfg, "moneyGap");
            }
            for id in &bars {
                self.bar(
                    &c,
                    data,
                    cfg,
                    player,
                    *id,
                    names[*id],
                    labels[*id],
                    0.0,
                    y + n(cfg, "barsY"),
                    column,
                );
                y += step;
            }
            bottom = y.max(weapon_y + side);
        } else if style == 1 && side > 0.0 {
            let extra = bars.len().saturating_sub(1);
            let stack = if extra > 0 {
                extra as f32 * step - n(cfg, "barGap")
            } else {
                0.0
            };
            let mut y = (side - stack).max(0.0);
            for id in bars.iter().skip(1) {
                self.bar(
                    &c,
                    data,
                    cfg,
                    player,
                    *id,
                    names[*id],
                    labels[*id],
                    side + gap,
                    y + n(cfg, "barsY"),
                    column,
                );
                y += step;
            }
            y = side + n(cfg, "moneyGap");
            if let Some(id) = bars.first() {
                y = side + n(cfg, "gap");
                self.bar(
                    &c,
                    data,
                    cfg,
                    player,
                    *id,
                    names[*id],
                    labels[*id],
                    0.0,
                    y + n(cfg, "barsY"),
                    width,
                );
                y += n(cfg, "barHeight") + n(cfg, "barOutline") + n(cfg, "moneyGap");
            }
            bottom = y + self.money(&c, data, cfg, player, 0.0, y, width);
        } else {
            let stack = if bars.is_empty() {
                0.0
            } else {
                bars.len() as f32 * step - n(cfg, "barGap")
            };
            let mut y = (side - stack).max(0.0);
            for id in &bars {
                self.bar(
                    &c,
                    data,
                    cfg,
                    player,
                    *id,
                    names[*id],
                    labels[*id],
                    side + gap,
                    y + n(cfg, "barsY"),
                    column,
                );
                y += step;
            }
            let bars_bottom = if bars.is_empty() {
                0.0
            } else {
                y - n(cfg, "barGap")
            };
            y = bars_bottom.max(side) + n(cfg, "moneyGap");
            bottom = y + self.money(&c, data, cfg, player, 0.0, y, width);
        }
        if b(cfg, "wanted") && n(player, "wanted") > 0.0 {
            bottom += 8.0;
            let x = if style == 2 {
                0.0
            } else {
                (width - 108.0).max(0.0)
            };
            let count = (n(player, "wanted") as usize).min(6);
            let (origin, scale) = self.bounds(
                data,
                "hud.wanted",
                "Уровень розыска",
                c.point(x, bottom),
                vec2(count as f32 * 18.0 - 4.0, 14.0),
                c.scale,
            );
            let star = c.child(origin, scale);
            for index in 0..count {
                let center = vec2(7.0 + index as f32 * 18.0, 7.0);
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(
                    star.point(center.x, center.y),
                    ink([1.0, 0.8, 0.2, 1.0], c.opacity),
                );
                for vertex in 0..10 {
                    let angle =
                        -std::f32::consts::FRAC_PI_2 + vertex as f32 * std::f32::consts::PI / 5.0;
                    let radius = if vertex % 2 == 0 { 7.0 } else { 3.0 };
                    mesh.colored_vertex(
                        star.point(
                            center.x + angle.cos() * radius,
                            center.y + angle.sin() * radius,
                        ),
                        ink([1.0, 0.8, 0.2, 1.0], c.opacity),
                    );
                }
                for vertex in 0..10u32 {
                    mesh.indices
                        .extend_from_slice(&[0, 1 + vertex, 1 + (vertex + 1) % 10]);
                }
                let outline = (0..10)
                    .map(|vertex| {
                        let angle = -std::f32::consts::FRAC_PI_2
                            + vertex as f32 * std::f32::consts::PI / 5.0;
                        let radius = if vertex % 2 == 0 { 7.0 } else { 3.0 };
                        star.point(
                            center.x + angle.cos() * radius,
                            center.y + angle.sin() * radius,
                        )
                    })
                    .collect();
                star.painter.add(mesh);
                star.painter.add(Shape::closed_line(
                    outline,
                    Stroke::new(star.scale, ink(rgba(cfg, "outlineColor"), c.opacity)),
                ));
            }
        }
        let union = self
            .elements
            .iter()
            .skip(hud_index + 1)
            .filter(|element| s(element, "id").starts_with("hud."))
            .fold(Rect::NOTHING, |rect, element| {
                rect.union(Rect::from_min_size(
                    pos2(n(element, "x"), n(element, "y")),
                    vec2(n(element, "width"), n(element, "height")),
                ))
            });
        if union.is_finite() {
            self.elements[hud_index]["x"] = json!(union.min.x);
            self.elements[hud_index]["y"] = json!(union.min.y);
            self.elements[hud_index]["width"] = json!(union.width());
            self.elements[hud_index]["height"] = json!(union.height());
        }
        let parent = self.elements.remove(hud_index);
        self.elements.push(parent);
    }

    #[allow(clippy::too_many_arguments)]
    fn bar(
        &mut self,
        parent: &Canvas<'_>,
        data: &Value,
        cfg: &Value,
        player: &Value,
        id: usize,
        key: &str,
        label: &str,
        x: f32,
        y: f32,
        width: f32,
    ) {
        let (origin, scale) = self.bounds(
            data,
            &format!("hud.{key}"),
            label,
            parent.point(x, y),
            vec2(width, n(cfg, "barHeight")),
            parent.scale,
        );
        let c = parent.child(origin, scale);
        let mut x = 0.0;
        let mut width = width;
        if let Some(source) = data["statusIcons"][id.to_string()].as_str() {
            if let Some(picture) =
                self.picture(c.painter.ctx(), &format!("status-{id}"), source, true)
            {
                let size = n(cfg, "statusIconSize");
                let reserved = size + n(cfg, "statusIconGap");
                let icon_x = if n(cfg, &format!("{key}IconSide")) == 0.0 {
                    x += reserved;
                    width -= reserved;
                    0.0
                } else {
                    width -= reserved;
                    x + width + n(cfg, "statusIconGap")
                };
                let w = size.min(size * picture.aspect);
                let h = w / picture.aspect;
                c.image(
                    &picture,
                    icon_x + (size - w) * 0.5,
                    (n(cfg, "barHeight") - h) * 0.5,
                    w,
                    h,
                    [1.0; 4],
                );
            }
        }
        let label = format!("{}%", n(player, key).round() as i32);
        let percent = b(cfg, &format!("{key}Value"));
        let font = n(cfg, "valueFont") as usize;
        let mut size = n(cfg, "valueSize") * if font == 4 { 0.72 } else { 1.0 };
        let mut maximum = c.measure("100%", size, font, true);
        if n(cfg, "valuePlacement") != 2.0 && maximum.x > width * 0.5 {
            size *= width * 0.5 / maximum.x;
            maximum = c.measure("100%", size, font, true);
        }
        let extent = c.measure(&label, size, font, true);
        let reserved = maximum.x + 6.0 + n(cfg, "valueOutline") * 2.0;
        let mut label_x = x + width * 0.5;
        if percent && n(cfg, "valuePlacement") != 2.0 {
            if n(cfg, "valuePlacement") == 0.0 {
                label_x = x + maximum.x * 0.5;
                width -= reserved;
                x += reserved;
            } else {
                width -= reserved;
                label_x = x + width + 6.0 + maximum.x * 0.5;
            }
        }
        width = width.max(2.0);
        let height = n(cfg, "barHeight");
        let outline = n(cfg, "barOutline");
        let rounding = n(cfg, "barRounding").min(height * 0.5);
        if outline > 0.0 {
            c.fill(
                x - outline,
                -outline,
                width + 2.0 * outline,
                height + 2.0 * outline,
                rgba(cfg, "outlineColor"),
                if rounding > 0.0 {
                    rounding + outline
                } else {
                    0.0
                },
            );
        }
        c.fill(x, 0.0, width, height, rgba(cfg, "barBackground"), rounding);
        let filled = width * (n(player, key) / 100.0).clamp(0.0, 1.0);
        if filled > 0.0 {
            c.fill(
                x,
                0.0,
                filled,
                height,
                rgba(cfg, &format!("{key}Color")),
                rounding.min(filled * 0.5),
            );
        }
        if percent {
            c.outlined(
                label_x,
                (height - extent.y) * 0.5,
                size,
                font,
                &label,
                rgba(cfg, "valueColor"),
                rgba(cfg, "valueOutlineColor"),
                n(cfg, "valueOutline"),
                true,
                true,
            );
        }
    }

    fn money(
        &mut self,
        parent: &Canvas<'_>,
        data: &Value,
        cfg: &Value,
        player: &Value,
        x: f32,
        y: f32,
        width: f32,
    ) -> f32 {
        if !b(cfg, "money") {
            return 0.0;
        }
        let amount = value(player, "money")
            .as_i64()
            .or_else(|| s(player, "money").parse().ok())
            .unwrap_or(0);
        let mut digits = amount.unsigned_abs().to_string();
        if b(cfg, "moneyZeros") && digits.len() < 8 {
            digits = format!("{:0>8}", digits);
        }
        if b(cfg, "moneyDots") {
            let mut pos = digits.len() as i32 - 3;
            while pos > 0 {
                digits.insert(pos as usize, '.');
                pos -= 3;
            }
        }
        let currency = match n(cfg, "currency") as u32 {
            0 => "$",
            1 => "₽",
            _ => "",
        };
        let text = format!("{}{currency}{digits}", if amount < 0 { "-" } else { "" });
        let font = n(cfg, "moneyFont") as usize;
        let mut size = n(cfg, "moneySize");
        let measured = parent.measure(&text, size, font, true);
        let space = (width - n(cfg, "moneyOutline") * 2.0).max(1.0);
        if measured.x > space {
            size *= space / measured.x;
        }
        let galley =
            parent
                .painter
                .layout_no_wrap(text, parent.font_id(font, size, true), Color32::WHITE);
        let bounds = galley.mesh_bounds;
        if !bounds.is_finite() || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return 0.0;
        }
        let height = bounds.height() / parent.scale + n(cfg, "moneyOutline") * 2.0;
        let occupied_width = if b(cfg, "moneyZeros") {
            width
        } else {
            bounds.width() / parent.scale + n(cfg, "moneyOutline") * 2.0
        };
        let (origin, scale) = self.bounds(
            data,
            "hud.money",
            "Деньги",
            parent.point(x, y),
            vec2(occupied_width, height),
            parent.scale,
        );
        let c = parent.child(origin, scale);
        let transformed = if (c.scale - parent.scale).abs() > 0.0001 {
            c.painter.layout_no_wrap(
                galley.job.text.clone(),
                c.font_id(font, size, true),
                Color32::WHITE,
            )
        } else {
            galley
        };
        let bounds = transformed.mesh_bounds;
        let stretch = if b(cfg, "moneyZeros") {
            space * c.scale / bounds.width().max(1.0)
        } else {
            1.0
        };
        let target = c.point(n(cfg, "moneyOutline"), n(cfg, "moneyOutline"));
        let effect = n(cfg, "moneyEffect") as i32;
        let phase = (crate::bridge::ticks() as f64
            / (1000.0 * n(cfg, "moneyEffectPeriod").max(0.5) as f64))
            .fract() as f32;
        let render = |dx: f32, dy: f32, color: [f32; 4], animate: bool| {
            let mut layer = (*transformed).clone();
            for row in &mut layer.rows {
                for vertex in &mut row.visuals.mesh.vertices {
                    vertex.pos.x = (vertex.pos.x - bounds.left()) * stretch;
                    vertex.pos.y -= bounds.top();
                    let mut tint = color;
                    if animate && effect > 0 {
                        tint = animated(
                            effect,
                            rgba(cfg, "moneyEffectFirst"),
                            rgba(cfg, "moneyEffectSecond"),
                            phase + vertex.pos.x / (space * c.scale).max(1.0),
                        );
                        tint[3] *= color[3];
                    }
                    vertex.color = ink(tint, c.opacity);
                }
                row.visuals.mesh_bounds = row.visuals.mesh.calc_bounds();
                row.rect = row.visuals.mesh_bounds;
            }
            layer.mesh_bounds =
                Rect::from_min_size(Pos2::ZERO, vec2(bounds.width() * stretch, bounds.height()));
            layer.rect = layer.mesh_bounds;
            c.painter.galley(
                target + vec2(dx, dy) * c.scale,
                std::sync::Arc::new(layer),
                Color32::WHITE,
            );
        };
        let outline = n(cfg, "moneyOutline");
        if outline > 0.0 {
            let steps = (outline * 8.0).ceil().max(8.0) as usize;
            for step in 0..steps {
                let angle = step as f32 * std::f32::consts::TAU / steps as f32;
                render(
                    angle.cos() * outline,
                    angle.sin() * outline,
                    rgba(cfg, "moneyOutlineColor"),
                    false,
                );
            }
        }
        render(0.0, 0.0, rgba(cfg, "moneyColor"), true);
        height
    }

    fn weapon(
        &mut self,
        parent: &Canvas<'_>,
        data: &Value,
        cfg: &Value,
        player: &Value,
        x: f32,
        y: f32,
        ammo: String,
    ) {
        let size = n(cfg, "weaponSize");
        let (origin, scale) = self.bounds(
            data,
            "hud.weapon",
            "Оружие и патроны",
            parent.point(x, y),
            vec2(size, size),
            parent.scale,
        );
        let c = parent.child(origin, scale);
        let id = (n(player, "weapon") as u32).to_string();
        let (key, source) = if let Some(source) = data["weaponImages"][&id].as_str() {
            (format!("weapon-custom-{id}"), source.to_owned())
        } else {
            let source = self
                .weapon_sources
                .get(&id)
                .or_else(|| self.weapon_sources.get("0"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            (format!("weapon-{id}"), source)
        };
        let rounding = n(cfg, "frameRounding").min(size * 0.5);
        let frame = n(cfg, "frameWidth");
        c.fill(
            0.0,
            0.0,
            size,
            size,
            rgba(cfg, "weaponBackground"),
            rounding,
        );
        let picture = self
            .picture(c.painter.ctx(), &key, &source, true)
            .or_else(|| {
                let fallback = self
                    .weapon_sources
                    .get(&id)
                    .or_else(|| self.weapon_sources.get("0"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                self.picture(
                    c.painter.ctx(),
                    &format!("weapon-fallback-{id}"),
                    &fallback,
                    true,
                )
            });
        if let Some(picture) = picture {
            let clipped = n(cfg, "weaponClip") == 1.0;
            let inset = if clipped {
                n(cfg, "weaponPadding")
                    .max(frame + n(cfg, "weaponOutline") + 1.0)
                    .min((size - 1.0) * 0.5)
            } else {
                n(cfg, "weaponPadding").min((size - 1.0) * 0.5)
            };
            let ammo_height = if ammo.is_empty() {
                0.0
            } else {
                n(cfg, "ammoSize") + 4.0
            };
            let available_width = (size - inset * 2.0).max(1.0);
            let available_height = (size - inset * 2.0 - ammo_height).max(1.0);
            let w = available_width.min(available_height * picture.aspect);
            let h = w / picture.aspect;
            let x = inset + (available_width - w) * 0.5;
            let y = inset + (available_height - h) * 0.5;
            let mut draw = c.child(c.origin, c.scale);
            if clipped {
                draw.painter = draw.painter.with_clip_rect(c.rect(
                    frame,
                    frame,
                    size - frame * 2.0,
                    size - frame * 2.0,
                ));
            }
            let image_rounding = if clipped {
                (rounding - inset).max(0.0).min(w.min(h) * 0.5)
            } else {
                0.0
            };
            let outline = n(cfg, "weaponOutline");
            if outline > 0.0 {
                for step in 0..8 {
                    let a = step as f32 * std::f32::consts::FRAC_PI_4;
                    draw.image_round(
                        &picture,
                        x + a.cos() * outline,
                        y + a.sin() * outline,
                        w,
                        h,
                        rgba(cfg, "outlineColor"),
                        image_rounding,
                    );
                }
            }
            draw.image_round(&picture, x, y, w, h, [1.0; 4], image_rounding);
        }
        if !ammo.is_empty() {
            let font = n(cfg, "ammoFont") as usize;
            let mut size_font = n(cfg, "ammoSize");
            let mut measured = c.measure(&ammo, size_font, font, true);
            let space = (size - frame * 2.0 - 8.0).max(1.0);
            if measured.x > space {
                size_font *= space / measured.x;
                measured = c.measure(&ammo, size_font, font, true);
            }
            c.outlined(
                size * 0.5,
                size - frame - 3.0 - measured.y,
                size_font,
                font,
                &ammo,
                rgba(cfg, "valueColor"),
                rgba(cfg, "valueOutlineColor"),
                n(cfg, "valueOutline"),
                true,
                true,
            );
        }
        c.stroke(
            frame * 0.5,
            frame * 0.5,
            size - frame,
            size - frame,
            rgba(cfg, "frameColor"),
            (rounding - frame * 0.5).max(0.0),
            frame,
        );
    }

    fn logo(&mut self, root: &Canvas<'_>, data: &Value, player: &Value, screen: Rect) {
        let cfg = &data["widgets"]["logo"];
        if n(cfg, "style") == 0.0 || n(cfg, "customMode") == 0.0 {
            return;
        }
        let server = if n(cfg, "server") > 0.0 {
            n(cfg, "server")
        } else {
            n(player, "server")
        } as u32;
        let server_source = if n(cfg, "customMode") == 2.0 {
            data["logoImages"][server.to_string()].as_str()
        } else {
            None
        };
        let Some(source) = server_source.or_else(|| data["logoImages"]["0"].as_str()) else {
            return;
        };
        let Some(picture) = self.picture(root.painter.ctx(), "custom-logo", source, true) else {
            return;
        };
        self.custom_logo_active = true;
        let hud = &data["widgets"]["playerHud"];
        let attached = b(cfg, "followHud") && n(hud, "style") > 0.0;
        let right = if attached {
            n(hud, "right")
        } else {
            n(cfg, "right")
        };
        let top = if attached {
            n(hud, "top").max(n(cfg, "top") + n(cfg, "height") * n(cfg, "scale") + n(cfg, "gap"))
                - n(cfg, "height") * n(cfg, "scale")
                - n(cfg, "gap")
        } else {
            n(cfg, "top")
        };
        let scale = n(cfg, "scale");
        let width = n(cfg, "width");
        let height = n(cfg, "height");
        let w = width.min(height * picture.aspect);
        let h = w / picture.aspect;
        let origin = pos2(
            (screen.right() - right - width * scale).max(screen.left()) + (width - w) * scale * 0.5,
            screen.top() + top + (height - h) * scale * 0.5,
        );
        let (origin, scale) = self.bounds(data, "logo", "Логотип", origin, vec2(w, h), scale);
        let mut c = root.child(origin, scale);
        c.opacity = n(cfg, "opacity");
        if n(cfg, "effect") > 0.0 {
            let mut mesh = egui::Mesh::with_texture(picture.id);
            let phase = (crate::bridge::ticks() as f64
                / (1000.0 * n(cfg, "effectPeriod").max(0.5) as f64))
                .fract() as f32;
            let uvs = [
                picture.uv.left_top(),
                picture.uv.right_top(),
                picture.uv.right_bottom(),
                picture.uv.left_bottom(),
            ];
            for (index, (x, y)) in [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
                .into_iter()
                .enumerate()
            {
                let color = animated(
                    n(cfg, "effect") as i32,
                    rgba(cfg, "effectFirst"),
                    rgba(cfg, "effectSecond"),
                    phase + x / w.max(1.0),
                );
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: c.point(x, y),
                    uv: uvs[index],
                    color: ink(color, c.opacity),
                });
            }
            mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
            c.painter.add(mesh);
        } else {
            c.image(&picture, 0.0, 0.0, w, h, [1.0; 4]);
        }
    }

    fn speedometer(&mut self, root: &Canvas<'_>, data: &Value, vehicle: &Value, screen: Rect) {
        let cfg = &data["widgets"]["speedometer"];
        let style = n(cfg, "style") as i32;
        if style == 0 {
            return;
        }
        let (width, height) = match style {
            1 => (346.0, 133.0),
            2 => (349.0, 125.0),
            _ => (260.0, 199.0),
        };
        let scale = n(cfg, "scale");
        let origin = pos2(
            screen.right() - width * scale - n(cfg, "right"),
            screen.bottom() - height * scale - n(cfg, "bottom"),
        );
        let (origin, scale) = self.bounds(
            data,
            "speedometer",
            "Спидометр",
            origin,
            vec2(width, height),
            scale,
        );
        let mut c = root.child(origin, scale);
        c.opacity = n(cfg, "opacity");
        c.font = n(cfg, "font") as usize;
        let dt = root.painter.ctx().input(|input| input.unstable_dt);
        self.display_speed +=
            (n(vehicle, "speed") - self.display_speed) * (1.0 - (-dt * 18.0).exp());
        if (self.display_speed - n(vehicle, "speed")).abs() < 0.05 {
            self.display_speed = n(vehicle, "speed");
        }
        let speed = self.display_speed;
        let ratio = (speed / n(cfg, "maxSpeed").max(1.0)).clamp(0.0, 1.0);
        let color = rgba(cfg, "color");
        let accent = rgba(cfg, "accent");
        let on = rgba(cfg, "onColor");
        let off = rgba(cfg, "offColor");
        let factor = if c.font == 4 { 0.72 } else { 1.0 };
        if style == 1 {
            c.fill(
                0.0,
                0.0,
                width,
                height,
                rgba(cfg, "background"),
                n(cfg, "rounding").clamp(0.0, 66.5),
            );
            let rounding = n(cfg, "rounding").clamp(0.0, 66.5);
            if rounding > 0.0 {
                let color = [0.19, 0.52, 0.71, 1.0];
                let thickness = n(cfg, "border").max(1.0);
                c.arc(rounding, rounding, rounding, 180.0, 270.0, color, thickness);
                c.line((rounding, 0.0), (width - rounding, 0.0), color, thickness);
                c.arc(
                    width - rounding,
                    rounding,
                    rounding,
                    270.0,
                    360.0,
                    color,
                    thickness,
                );
            } else {
                c.fill(
                    0.0,
                    0.0,
                    width,
                    n(cfg, "border").max(1.0),
                    [0.19, 0.52, 0.71, 1.0],
                    0.0,
                );
            }
            c.text(
                8.0,
                5.0,
                24.0 * factor,
                &format!("{} :КМ/Ч", speed.round() as u32),
                color,
                false,
            );
            if b(cfg, "mileage") {
                let text = mileage(s(vehicle, "mileage"), 7);
                let extent = c.measure(&text, 24.0 * factor, c.font, true);
                c.text(
                    338.0 - extent.x * 0.5,
                    5.0,
                    24.0 * factor,
                    &text,
                    color,
                    true,
                );
            }
            for i in 0..36 {
                c.fill(
                    8.0 + i as f32 * 9.285714,
                    34.0,
                    5.0,
                    20.0,
                    if (i as f32) < ratio * 36.0 {
                        accent
                    } else {
                        [0.68, 0.67, 0.72, 1.0]
                    },
                    0.0,
                );
            }
            if b(cfg, "fuel") {
                c.text(8.0, 65.0, 25.0 * factor, "FUEL:", color, false);
                c.text(
                    112.0,
                    65.0,
                    25.0 * factor,
                    &format!("{}", n(vehicle, "fuel").round() as u32),
                    accent,
                    false,
                );
            }
            if b(cfg, "indicators") {
                c.text(
                    196.0,
                    64.0,
                    28.0 * factor,
                    "M",
                    if b(vehicle, "engine") { on } else { off },
                    false,
                );
                c.text(
                    262.0,
                    64.0,
                    28.0 * factor,
                    "L",
                    if b(vehicle, "lights") { on } else { off },
                    false,
                );
                c.text(
                    314.0,
                    64.0,
                    28.0 * factor,
                    "B",
                    if b(vehicle, "belt") {
                        on
                    } else {
                        [0.55, 0.55, 0.6, 1.0]
                    },
                    false,
                );
                c.text(8.0, 99.0, 21.0 * factor, "ALARM:", color, false);
                c.text(
                    112.0,
                    99.0,
                    21.0 * factor,
                    if b(vehicle, "alarm") { "ON" } else { "OFF" },
                    if b(vehicle, "alarm") { on } else { off },
                    false,
                );
                c.text(
                    194.0,
                    96.0,
                    26.0 * factor,
                    if b(vehicle, "locked") {
                        "CLOSE"
                    } else {
                        "OPEN"
                    },
                    if b(vehicle, "locked") { off } else { on },
                    false,
                );
                c.circle(
                    330.0,
                    107.0,
                    8.0,
                    if (b(vehicle, "left") || b(vehicle, "right"))
                        && (crate::bridge::ticks() / 450) % 2 == 0
                    {
                        accent
                    } else {
                        color
                    },
                );
            }
        } else if style == 2 {
            self.analog(&c, cfg, vehicle, speed, ratio, factor);
        } else {
            self.circular(&c, cfg, vehicle, speed, ratio, factor);
        }
    }

    fn speed_picture(
        &mut self,
        c: &Canvas<'_>,
        name: &str,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [f32; 4],
    ) {
        let source = if let Some(index) = name
            .strip_prefix("digit")
            .and_then(|s| s.parse::<usize>().ok())
        {
            self.speedometer_sources["digits"][index].as_str()
        } else {
            self.speedometer_sources[name].as_str()
        }
        .unwrap_or("")
        .to_owned();
        if let Some(picture) =
            self.picture(c.painter.ctx(), &format!("speed-{name}"), &source, false)
        {
            c.image(&picture, x, y, width, height, color);
        }
    }

    fn analog(
        &mut self,
        c: &Canvas<'_>,
        cfg: &Value,
        v: &Value,
        speed: f32,
        ratio: f32,
        factor: f32,
    ) {
        let color = rgba(cfg, "color");
        let accent = rgba(cfg, "accent");
        let on = rgba(cfg, "onColor");
        let off = rgba(cfg, "offColor");
        let source = self.speedometer_sources["speedometer_bg"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        if let Some(picture) = self.picture(c.painter.ctx(), "speed-speedometer_bg", &source, false)
        {
            c.image_round(
                &picture,
                0.0,
                0.0,
                349.0,
                125.0,
                [1.0; 4],
                n(cfg, "rounding"),
            );
        }
        let source = self.speedometer_sources["arrow"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        if let Some(picture) = self.picture(c.painter.ctx(), "speed-arrow", &source, false) {
            c.rotated(&picture, 62.5, 62.5, 125.0, 180.0 + 360.0 * ratio);
        }
        let text = format!("{}", speed.round() as u32);
        let distance = mileage(s(v, "mileage"), 6);
        if c.font == 4 {
            for (i, digit) in text.chars().enumerate() {
                self.speed_picture(
                    c,
                    &format!("digit{digit}"),
                    62.5 - text.len() as f32 * 5.5 + i as f32 * 11.0,
                    54.0,
                    11.0,
                    12.0,
                    [1.0; 4],
                );
            }
            if b(cfg, "mileage") {
                for (i, digit) in distance.chars().enumerate() {
                    self.speed_picture(
                        c,
                        &format!("digit{digit}"),
                        143.0 + i as f32 * 12.0,
                        17.0,
                        12.0,
                        14.0,
                        [1.0; 4],
                    );
                }
            }
        } else {
            c.text(62.5, 51.0, 17.0, &text, color, true);
            if b(cfg, "mileage") {
                c.text(143.0, 17.0, 19.0, &distance, color, false);
            }
        }
        if b(cfg, "mileage") {
            c.text(220.0, 17.0, 13.0 * factor, "КМ", accent, false);
        }
        if b(cfg, "fuel") {
            self.speed_picture(c, "fuel_icon", 150.0, 51.0, 14.0, 16.0, accent);
            c.text(
                157.0,
                69.0,
                14.0 * factor,
                &format!("{}", n(v, "fuel").round() as u32),
                color,
                true,
            );
            let fuel = (n(v, "fuel") / n(v, "maxFuel").max(1.0)).clamp(0.0, 1.0);
            for i in 0..8 {
                c.fill(
                    180.0 + i as f32 * 16.0,
                    50.0,
                    5.0,
                    26.0,
                    if (i as f32) < fuel * 8.0 {
                        accent
                    } else {
                        [0.12, 0.12, 0.13, 1.0]
                    },
                    0.0,
                );
            }
        }
        if b(cfg, "indicators") {
            for (name, key, x) in [
                ("light_icon", "lights", 141.0),
                ("engine_icon", "engine", 180.0),
                ("belt_icon", "belt", 219.0),
                ("door_icon", "locked", 260.0),
            ] {
                self.speed_picture(
                    c,
                    name,
                    x,
                    94.0,
                    21.0,
                    21.0,
                    if b(v, key) { on } else { off },
                );
            }
        }
    }

    fn circular(
        &mut self,
        c: &Canvas<'_>,
        cfg: &Value,
        v: &Value,
        speed: f32,
        ratio: f32,
        factor: f32,
    ) {
        let fuel = (n(v, "fuel") / n(v, "maxFuel").max(1.0)).clamp(0.0, 1.0);
        let color = rgba(cfg, "color");
        let accent = rgba(cfg, "accent");
        let dim = [color[0], color[1], color[2], color[3] * 0.4];
        let (cx, cy) = (160.0, 94.0);
        if rgba(cfg, "background")[3] > 0.0 {
            c.circle(cx, cy, 77.0, rgba(cfg, "background"));
        }
        let mut glow = egui::Mesh::default();
        for ring in 0..24 {
            for part in 0..48 {
                let middle = (part as f32 + 0.5) / 48.0;
                let alpha =
                    (ring as f32 + 0.5) / 24.0 * 0.7 * (middle * std::f32::consts::PI).sin();
                let r0 = 64.0 + ring as f32;
                let r1 = r0 + 1.0;
                let a0 = (90.0 + part as f32 * 3.75).to_radians();
                let a1 = a0 + 3.75f32.to_radians();
                let base = glow.vertices.len() as u32;
                for (a, r) in [(a0, r0), (a0, r1), (a1, r1), (a1, r0)] {
                    glow.colored_vertex(
                        c.point(96.0 + a.cos() * r, 94.0 + a.sin() * r),
                        ink([1.0, 0.49, 0.15, alpha], c.opacity),
                    );
                }
                glow.indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 2,
                    base,
                    base + 2,
                    base + 3,
                ]);
            }
        }
        c.painter.add(glow);
        c.arc(cx, cy, 65.0, 124.0, 405.0, [0.85, 0.9, 0.9, 0.13], 1.5);
        for a in [180.0f32, 220.0, 270.0, 328.0, 360.0] {
            let a = a.to_radians();
            c.line(
                (cx + a.cos() * 49.0, cy + a.sin() * 49.0),
                (cx + a.cos() * 62.0, cy + a.sin() * 62.0),
                [0.9, 0.94, 0.93, 0.8],
                1.2,
            );
        }
        if ratio > 0.0 {
            c.arc(cx, cy, 65.0, 124.0, 124.0 + ratio * 273.0, accent, 3.0);
        }
        c.arc(cx, cy, 76.0, 181.0, 222.0, [1.0, 0.82, 0.1, 1.0], 1.7);
        let a = 222f32.to_radians();
        c.circle(cx + a.cos() * 76.0, cy + a.sin() * 76.0, 2.0, color);
        c.arc(cx, cy, 78.0, 0.0, 37.0, [1.0, 0.67, 0.2, 0.85], 1.7);
        let a = 37f32.to_radians();
        c.circle(cx + a.cos() * 78.0, cy + a.sin() * 78.0, 2.0, color);
        c.text(
            cx,
            68.0,
            33.0 * factor,
            &format!("{}", speed.round() as u32),
            color,
            true,
        );
        c.text(cx, 104.0, 11.5 * factor, "км/ч", accent, true);
        if b(cfg, "mileage") {
            c.text(
                cx,
                116.0,
                10.0 * factor,
                &mileage(s(v, "mileage"), 6),
                color,
                true,
            );
        }
        if b(cfg, "indicators") {
            let lock = if b(v, "locked") { color } else { dim };
            c.fill(58.0, 36.0, 8.0, 7.0, lock, 0.0);
            c.stroke(60.0, 30.0, 4.0, 8.0, lock, 2.0, 1.2);
            self.speed_picture(
                c,
                "light_icon",
                25.0,
                59.0,
                13.0,
                13.0,
                if b(v, "lights") { color } else { dim },
            );
            self.speed_picture(c, "door_icon", 18.0, 89.0, 14.0, 14.0, lock);
            let engine = if b(v, "engine") { color } else { dim };
            c.arc(33.0, 125.0, 3.0, 0.0, 360.0, engine, 1.2);
            c.line((35.0, 127.0), (42.0, 135.0), engine, 1.5);
            c.line((39.0, 132.0), (42.0, 129.0), engine, 1.5);
            let radius = 5.2;
            c.painter.circle_stroke(
                c.point(82.0, 106.0),
                radius * 0.56 * c.scale,
                Stroke::new(radius * 0.4 * c.scale, ink(engine, c.opacity)),
            );
            for tooth in 0..8 {
                let angle = tooth as f32 * std::f32::consts::FRAC_PI_4;
                let (sin, cos) = angle.sin_cos();
                let vertices = [(0.62, -0.18), (1.0, -0.18), (1.0, 0.18), (0.62, 0.18)]
                    .into_iter()
                    .map(|(radial, tangent)| {
                        c.point(
                            82.0 + radius * (radial * cos - tangent * sin),
                            106.0 + radius * (radial * sin + tangent * cos),
                        )
                    })
                    .collect();
                c.painter.add(Shape::convex_polygon(
                    vertices,
                    ink(engine, c.opacity),
                    Stroke::NONE,
                ));
            }
            c.painter.circle_stroke(
                c.point(89.0, 103.0),
                1.8 * c.scale,
                Stroke::new(1.1 * c.scale, ink(engine, c.opacity)),
            );
            let key = if b(v, "key") || b(v, "hasKey") {
                color
            } else {
                dim
            };
            for i in 0..3 {
                let a = i as f32 * std::f32::consts::PI / 3.0;
                c.line(
                    (237.0 - a.cos() * 4.0, 83.0 - a.sin() * 4.0),
                    (237.0 + a.cos() * 4.0, 83.0 + a.sin() * 4.0),
                    key,
                    1.8,
                );
            }
            let pulse = 0.25
                + 0.75
                    * (0.5
                        - 0.5
                            * ((crate::bridge::ticks() % 1000) as f32 / 1000.0
                                * std::f32::consts::TAU)
                                .cos());
            for (x, key, right) in [(131.0, "left", false), (187.0, "right", true)] {
                let mut color = accent;
                color[3] *= if b(v, key) { pulse } else { 0.25 };
                let sign = if right { -1.0 } else { 1.0 };
                c.line((x, 137.0), (x + 23.0 * sign, 137.0), color, 2.0);
                c.line((x, 137.0), (x + 8.0 * sign, 131.0), color, 2.0);
                c.line((x, 137.0), (x + 8.0 * sign, 143.0), color, 2.0);
            }
        }
        if b(cfg, "fuel") {
            self.speed_picture(c, "fuel_icon", 60.0, 154.0, 12.0, 15.0, color);
            c.text(
                78.0,
                154.0,
                14.0 * factor,
                &format!("{}%", (fuel * 100.0).round() as u32),
                color,
                false,
            );
        }
    }
}

fn mileage(value: &str, minimum: usize) -> String {
    let mut digits: String = value.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        digits.push('0');
    }
    if digits.len() < minimum {
        digits = format!("{:0>width$}", digits, width = minimum);
    }
    if digits.len() > 9 {
        digits = digits[digits.len() - 9..].to_owned();
    }
    digits
}
