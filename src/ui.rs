mod controls;

use crate::assets;
use crate::bridge::{BridgeStatus, ElementBounds, PlayerHudState, VehicleState};
use crate::schema::{self, Kind};
use crate::settings::Config;
use crate::widgets::Widgets;
use base64::Engine;
use controls::{
    card, checkbox_value, color_control, font_picker, rgba, spec_control, switch_value, tabs,
};
use egui::{Align2, Color32, FontFamily, FontId, Id, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const SECTIONS: [&str; 11] = [
    "Диалоги",
    "Радар",
    "Безопасная зона",
    "Чат",
    "Часы",
    "HUD игрока",
    "Логотип",
    "Спидометр",
    "Сообщения",
    "Расположение",
    "Профили",
];
const GROUP_SECTION: [&str; 11] = [
    "",
    "radar",
    "safety",
    "chat",
    "clock",
    "playerHud",
    "logo",
    "speedometer",
    "gametext",
    "",
    "",
];
const INK: Color32 = Color32::from_rgb(222, 224, 235);
const MUTED: Color32 = Color32::from_rgb(115, 120, 135);
const ACCENT: Color32 = Color32::from_rgb(139, 127, 255);

struct ImageResult {
    kind: usize,
    id: i64,
    source: String,
    image: egui::ColorImage,
}

pub struct App {
    pub config: Config,
    pub revision: u64,
    pub menu_open: bool,
    pub preview_request: Option<i32>,
    pub refresh_requested: bool,
    pub widgets: Widgets,
    pub backdrop: Option<egui::TextureHandle>,
    root: PathBuf,
    section: usize,
    tabs: [usize; 3],
    font_filter: String,
    search: String,
    message: String,
    message_until: Instant,
    save_due: Option<Instant>,
    styled: bool,
    fonts: egui::FontDefinitions,
    loaded_fonts: BTreeSet<(usize, bool)>,
    menu_rect: Option<Rect>,
    menu_position: Option<Pos2>,
    elements: Vec<ElementBounds>,
    selected_element: String,
    dragged_element: String,
    image_ids: [i64; 3],
    image_loaded_ids: [i64; 3],
    image_inputs: [String; 3],
    image_receiver: Option<Receiver<Result<ImageResult, String>>>,
    image_previews: BTreeMap<(usize, i64), egui::TextureHandle>,
}

impl App {
    pub fn new(root: &Path) -> Self {
        let (mut config, message) = match Config::load(root) {
            Ok(config) => (config, String::new()),
            Err(error) => (
                Config::default(),
                format!("Не удалось загрузить настройки: {error}"),
            ),
        };
        config.set_font_names(&assets::font_names());
        Self {
            config,
            revision: 1,
            menu_open: true,
            preview_request: None,
            refresh_requested: false,
            widgets: Widgets::new(),
            backdrop: None,
            root: root.to_owned(),
            section: 0,
            tabs: [0; 3],
            font_filter: String::new(),
            search: String::new(),
            message,
            message_until: Instant::now() + Duration::from_secs(6),
            save_due: None,
            styled: false,
            fonts: egui::FontDefinitions::default(),
            loaded_fonts: BTreeSet::new(),
            menu_rect: None,
            menu_position: None,
            elements: Vec::new(),
            selected_element: String::new(),
            dragged_element: String::new(),
            image_ids: [0; 3],
            image_loaded_ids: [-1; 3],
            image_inputs: std::array::from_fn(|_| String::new()),
            image_receiver: None,
            image_previews: BTreeMap::new(),
        }
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.config.save(&self.root)?;
        self.save_due = None;
        Ok(())
    }

    pub fn take_preview(&mut self) -> Option<i32> {
        self.preview_request.take()
    }

    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.save_due = Some(Instant::now() + Duration::from_millis(350));
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.message = text.into();
        self.message_until = Instant::now() + Duration::from_secs(6);
    }

    fn initialize_fonts(&mut self, ctx: &egui::Context) {
        let mut requested = BTreeSet::new();
        for group in ["clock", "speedometer", "playerHud"] {
            let keys: &[&str] = if group == "playerHud" {
                &["moneyFont", "valueFont", "ammoFont"]
            } else {
                &["font"]
            };
            for key in keys {
                let index = self.config.data["widgets"][group][key]
                    .as_u64()
                    .unwrap_or(0) as usize;
                let bold = if group == "clock" {
                    self.config.data["widgets"][group]["bold"]
                        .as_bool()
                        .unwrap_or(true)
                } else {
                    true
                };
                requested.insert((index, bold));
            }
        }
        let mut changed = false;
        if !self.fonts.font_data.contains_key("MenuRegular") {
            if let Ok(bytes) = assets::menu_font_bytes(false) {
                let name = "MenuRegular".to_owned();
                self.fonts.font_data.insert(
                    name.clone(),
                    egui::FontData::from_owned(bytes.as_ref().clone()).into(),
                );
                self.fonts
                    .families
                    .insert(FontFamily::Proportional, vec![name]);
                changed = true;
            }
        }
        for (index, bold) in requested {
            if self.loaded_fonts.contains(&(index, bold)) {
                continue;
            }
            if let Ok(bytes) = assets::font_bytes(index, bold) {
                self.loaded_fonts.insert((index, bold));
                let name = assets::font_name(index);
                let name = if bold { format!("{name} Bold") } else { name };
                self.fonts.font_data.insert(
                    name.clone(),
                    egui::FontData::from_owned(bytes.as_ref().clone()).into(),
                );
                self.fonts
                    .families
                    .insert(FontFamily::Name(name.clone().into()), vec![name.clone()]);
                changed = true;
            }
        }
        if changed {
            ctx.set_fonts(self.fonts.clone());
        }
    }

    fn style(&mut self, ctx: &egui::Context) {
        if self.styled {
            return;
        }
        self.styled = true;
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = Color32::TRANSPARENT;
        style.visuals.window_fill = Color32::from_rgba_unmultiplied(17, 17, 23, 252);
        style.visuals.window_stroke =
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(48, 51, 64, 140));
        style.visuals.window_corner_radius = 14.into();
        style.visuals.menu_corner_radius = 8.into();
        style.visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(20, 22, 28, 230);
        style.visuals.code_bg_color = Color32::from_rgba_unmultiplied(20, 22, 28, 230);
        style.visuals.faint_bg_color = Color32::from_white_alpha(5);
        style.visuals.widgets.noninteractive.bg_fill =
            Color32::from_rgba_unmultiplied(14, 15, 19, 140);
        style.visuals.widgets.noninteractive.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(48, 51, 64, 140));
        style.visuals.widgets.noninteractive.corner_radius = 9.into();
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgba_unmultiplied(20, 22, 28, 230);
        style.visuals.widgets.inactive.weak_bg_fill =
            Color32::from_rgba_unmultiplied(31, 31, 46, 242);
        style.visuals.widgets.inactive.bg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(48, 51, 64, 140));
        style.visuals.widgets.inactive.corner_radius = 5.into();
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(33, 36, 48);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(59, 54, 92);
        style.visuals.widgets.hovered.corner_radius = 5.into();
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(43, 41, 64);
        style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(84, 74, 140);
        style.visuals.widgets.active.corner_radius = 5.into();
        style.visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(48, 43, 79, 204);
        style.visuals.selection.stroke = Stroke::new(1.0_f32, INK);
        style.visuals.hyperlink_color = ACCENT;
        style.spacing.window_margin = egui::Margin::symmetric(18, 16);
        style.spacing.menu_margin = egui::Margin::symmetric(18, 16);
        style.spacing.item_spacing = vec2(12.0, 10.0);
        style.spacing.button_padding = vec2(10.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.slider_width = 230.0;
        style.spacing.scroll.bar_width = 7.0;
        style.spacing.scroll.floating_width = 7.0;
        style.spacing.scroll.dormant_background_opacity = 0.0;
        style.spacing.scroll.active_background_opacity = 0.0;
        style.spacing.scroll.interact_background_opacity = 0.0;
        for font in style.text_styles.values_mut() {
            *font = FontId::proportional(16.0);
        }
        ctx.set_style(style);
    }

    fn poll_image(&mut self, ctx: &egui::Context) {
        let result = self
            .image_receiver
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(value) => Some(value),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Чтение картинки прервано".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        let Some(result) = result else { return };
        self.image_receiver = None;
        match result {
            Ok(image) => {
                let key = image_map(image.kind);
                let mut next = self.config.data.clone();
                next[key][image.id.to_string()] = json!(image.source);
                if image.kind == 2 {
                    if next["widgets"]["logo"]["customMode"].as_i64().unwrap_or(0) == 0 {
                        next["widgets"]["logo"]["customMode"] = json!(1);
                    }
                    if next["widgets"]["logo"]["style"].as_i64().unwrap_or(0) == 0 {
                        next["widgets"]["logo"]["style"] = json!(1);
                    }
                }
                match self.config.apply(&next) {
                    Ok(()) => {
                        self.image_previews.insert(
                            (image.kind, image.id),
                            ctx.load_texture(
                                format!("custom-preview-{}-{}", image.kind, image.id),
                                image.image,
                                egui::TextureOptions::LINEAR,
                            ),
                        );
                        self.changed();
                        self.notify("Картинка применена");
                    }
                    Err(error) => self.notify(error),
                }
            }
            Err(error) => self.notify(error),
        }
    }

    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        status: &BridgeStatus,
        vehicle: &VehicleState,
        player: &PlayerHudState,
    ) {
        self.style(ctx);
        self.initialize_fonts(ctx);
        self.poll_image(ctx);
        let vehicle_json = serde_json::to_value(vehicle).unwrap_or(Value::Null);
        let player_json = serde_json::to_value(player).unwrap_or(Value::Null);
        if self.widgets.draw(
            ctx,
            &mut self.config,
            &vehicle_json,
            &player_json,
            self.menu_open,
        ) {
            self.changed();
        }
        self.elements.clear();
        if let Some(view) = status.views.first() {
            let screen = ctx.screen_rect();
            if view.width > 0.0 && view.height > 0.0 {
                let scale = vec2(screen.width() / view.width, screen.height() / view.height);
                for element in &view.elements {
                    let mut element = element.clone();
                    element.x = screen.left() + element.x * scale.x;
                    element.y = screen.top() + element.y * scale.y;
                    element.width *= scale.x;
                    element.height *= scale.y;
                    self.elements.push(element);
                }
            }
        }
        for element in &self.widgets.elements {
            if let Ok(element) = serde_json::from_value::<ElementBounds>(element.clone()) {
                self.elements.push(element);
            }
        }
        if self.menu_open {
            self.draw_menu(ctx, status, player);
            self.edit_layout(ctx);
        } else {
            self.menu_rect = None;
            self.dragged_element.clear();
        }
        if self.save_due.is_some_and(|when| Instant::now() >= when) {
            if let Err(error) = self.flush() {
                self.save_due = Some(Instant::now() + Duration::from_secs(5));
                self.notify(format!("Не удалось сохранить настройки: {error}"));
            }
        }
        if self.menu_open {
            draw_software_cursor(ctx);
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn draw_menu(&mut self, ctx: &egui::Context, status: &BridgeStatus, player: &PlayerHudState) {
        let screen = ctx.screen_rect();
        let default_size = vec2(
            1080.0_f32.min(screen.width() - 24.0),
            760.0_f32.min(screen.height() - 24.0),
        );
        let position = *self
            .menu_position
            .get_or_insert_with(|| screen.center() - default_size * 0.5);
        let mut open = self.menu_open;
        let shown = egui::Window::new("SampStyle###SampStyleMenu")
            .id(Id::new("SampStyleMenu"))
            .title_bar(false)
            .collapsible(false)
            .resizable(true)
            .movable(false)
            .current_pos(position)
            .default_size(default_size)
            .min_size(vec2(
                860.0_f32.min(screen.width() - 24.0),
                580.0_f32.min(screen.height() - 24.0),
            ))
            .max_size(vec2(1600.0, 1200.0))
            .frame(egui::Frame::NONE.inner_margin(0))
            .open(&mut open)
            .show(ctx, |ui| {
                let size = ui.available_size();
                let size = vec2(size.x.clamp(700.0, 1600.0), size.y.clamp(500.0, 1200.0));
                let outer = Rect::from_min_size(ui.min_rect().min, size);
                ui.allocate_space(size);
                let painter = ui.painter();
                painter.rect_filled(
                    outer.translate(vec2(5.0, 10.0)),
                    14.0,
                    Color32::from_black_alpha(70),
                );
                if let Some(backdrop) = &self.backdrop {
                    let uv = Rect::from_min_max(
                        pos2(
                            (outer.left() - screen.left()) / screen.width(),
                            (outer.top() - screen.top()) / screen.height(),
                        ),
                        pos2(
                            (outer.right() - screen.left()) / screen.width(),
                            (outer.bottom() - screen.top()) / screen.height(),
                        ),
                    );
                    painter.add(
                        egui::epaint::RectShape::filled(outer, 14.0, Color32::WHITE)
                            .with_texture(backdrop.id(), uv),
                    );
                }
                painter.rect_filled(
                    outer,
                    14.0,
                    Color32::from_rgba_unmultiplied(15, 16, 23, 224),
                );
                painter.rect_filled(
                    Rect::from_min_max(outer.min, pos2(outer.left() + 214.0, outer.bottom())),
                    egui::CornerRadius {
                        nw: 14,
                        sw: 14,
                        ne: 0,
                        se: 0,
                    },
                    Color32::from_rgba_unmultiplied(11, 12, 18, 175),
                );
                painter.rect_stroke(
                    outer,
                    14.0,
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(71, 72, 90, 80)),
                    egui::StrokeKind::Inside,
                );
                painter.line_segment(
                    [
                        pos2(outer.left() + 214.0, outer.top() + 76.0),
                        pos2(outer.right(), outer.top() + 76.0),
                    ],
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(74, 74, 94, 55)),
                );
                painter.line_segment(
                    [
                        pos2(outer.left() + 214.0, outer.top() + 76.0),
                        pos2(outer.left() + 214.0, outer.bottom()),
                    ],
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(74, 74, 94, 55)),
                );
                let mark = outer.min + vec2(30.0, 38.0);
                let points = [
                    mark + vec2(-8.0, 0.0),
                    mark + vec2(-3.0, 0.0),
                    mark + vec2(0.0, -9.0),
                    mark + vec2(4.0, 8.0),
                    mark + vec2(8.0, 0.0),
                ];
                painter.add(egui::Shape::line(
                    points.to_vec(),
                    Stroke::new(2.0_f32, ACCENT),
                ));
                painter.text(
                    outer.min + vec2(52.0, 27.0),
                    Align2::LEFT_TOP,
                    "SAMP STYLE",
                    FontId::proportional(17.0),
                    Color32::from_rgb(232, 231, 243),
                );
                painter.text(
                    outer.min + vec2(238.0, 28.0),
                    Align2::LEFT_TOP,
                    "Настройка интерфейса",
                    FontId::proportional(16.0),
                    Color32::from_rgb(128, 129, 148),
                );
                let move_rect = Rect::from_min_max(
                    outer.min + vec2(224.0, 12.0),
                    pos2(outer.right() - 85.0, outer.top() + 65.0),
                );
                let move_response = ui.interact(move_rect, Id::new("move-menu"), Sense::drag());
                if move_response.dragged() {
                    self.menu_position = Some(position + ctx.input(|input| input.pointer.delta()));
                }
                ui.allocate_new_ui(
                    egui::UiBuilder::new().max_rect(Rect::from_min_size(
                        pos2(outer.right() - 70.0, outer.top() + 22.0),
                        vec2(38.0, 30.0),
                    )),
                    |ui| {
                        if ui.add_sized([38.0, 30.0], egui::Button::new("X")).clicked() {
                            self.menu_open = false;
                        }
                    },
                );
                let nav = Rect::from_min_max(
                    outer.min + vec2(12.0, 88.0),
                    pos2(outer.left() + 202.0, outer.bottom() - 58.0),
                );
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(nav), |ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("navigation")
                        .show(ui, |ui| {
                            for (index, section) in SECTIONS.iter().enumerate() {
                                if index == 0 || index == 4 || index == 9 {
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(match index {
                                            0 => "ИНТЕРФЕЙС",
                                            4 => "ЭЛЕМЕНТЫ HUD",
                                            _ => "НАСТРОЙКИ",
                                        })
                                        .small()
                                        .color(MUTED),
                                    );
                                    ui.add_space(4.0);
                                }
                                let (rect, response) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 36.0),
                                    Sense::click(),
                                );
                                let active = index == self.section;
                                if active || response.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        6.0,
                                        if active {
                                            Color32::from_rgba_unmultiplied(67, 57, 113, 100)
                                        } else {
                                            Color32::from_rgba_unmultiplied(72, 72, 87, 48)
                                        },
                                    );
                                }
                                navigation_icon(
                                    ui.painter(),
                                    rect.min + vec2(15.0, 18.0),
                                    index,
                                    if active {
                                        Color32::from_rgb(154, 143, 255)
                                    } else {
                                        Color32::from_rgb(127, 129, 145)
                                    },
                                );
                                ui.painter().text(
                                    rect.min + vec2(36.0, 10.0),
                                    Align2::LEFT_TOP,
                                    *section,
                                    FontId::proportional(16.0),
                                    if active {
                                        Color32::from_rgb(238, 235, 255)
                                    } else {
                                        Color32::from_rgb(169, 170, 185)
                                    },
                                );
                                if response.clicked() {
                                    self.section = index;
                                }
                            }
                        });
                });
                let content =
                    Rect::from_min_max(outer.min + vec2(238.0, 94.0), outer.max - vec2(24.0, 60.0));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content), |ui| {
                    let title = schema::GROUPS
                        .iter()
                        .find(|group| group.key == GROUP_SECTION[self.section])
                        .map(|group| group.label)
                        .unwrap_or(SECTIONS[self.section]);
                    ui.label(title);
                    ui.add_space(7.0);
                    egui::ScrollArea::vertical()
                        .id_salt(("content", self.section))
                        .auto_shrink([false, false])
                        .show(ui, |ui| match self.section {
                            0 => self.dialogs_page(ui),
                            5 => self.player_hud_page(ui, player),
                            6 => self.logo_page(ui, player),
                            9 => self.layout_page(ui),
                            10 => self.profiles_page(ui, status),
                            section => self.widget_page(ui, GROUP_SECTION[section]),
                        });
                });
                ui.painter().text(
                    outer.min + vec2(22.0, size.y - 32.0),
                    Align2::LEFT_TOP,
                    "Insert - закрыть",
                    FontId::proportional(16.0),
                    MUTED,
                );
                let (footer, color) =
                    if !self.message.is_empty() && Instant::now() < self.message_until {
                        (self.message.as_str(), INK)
                    } else if !status.error.is_empty() {
                        (
                            "Переподключение к игровому интерфейсу...",
                            Color32::from_rgb(255, 166, 102),
                        )
                    } else {
                        (
                            "ЛКМ - перемещение   |   Колесо - размер   |   Автосохранение в INI",
                            MUTED,
                        )
                    };
                ui.painter().text(
                    pos2(outer.left() + 238.0, outer.bottom() - 32.0),
                    Align2::LEFT_TOP,
                    footer,
                    FontId::proportional(16.0),
                    color,
                );
            });
        if let Some(window) = shown {
            self.menu_rect = Some(window.response.rect);
        }
        self.menu_open &= open;
    }

    fn dialogs_page(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if self.config.data["enabled"].as_bool().unwrap_or(false) {
                    "Стиль SAMP включён"
                } else {
                    "Включить стиль SAMP"
                })
                .clicked()
            {
                self.config.data["enabled"] = json!(true);
                changed = true;
            }
            if ui.button("Вернуть исходное оформление").clicked() {
                self.config.reset_dialogs();
                changed = true;
            }
        });
        tabs(ui, &mut self.tabs[0], &["Палитра", "Размеры", "Текст"]);
        match self.tabs[0] {
            0 => {
                ui.columns(2, |columns| {
                    for spec in schema::PALETTE {
                        changed |= color_control(
                            &mut columns[0],
                            spec.label,
                            &mut self.config.data["colors"][spec.key],
                        );
                    }
                    dialog_preview(&mut columns[1], &self.config.data);
                    if columns[1].button("Стандартная палитра SAMP").clicked() {
                        for spec in schema::PALETTE {
                            self.config.data["colors"][spec.key] = json!(spec.initial);
                        }
                        changed = true;
                    }
                    columns[1]
                        .label("Изменения видны в игре сразу. Insert скрывает меню настроек.");
                });
            }
            1 => {
                for spec in schema::DIMENSIONS {
                    let mut value = self.config.data["sizes"][spec.key]
                        .as_f64()
                        .unwrap_or(spec.initial);
                    if ui
                        .add(
                            egui::Slider::new(&mut value, spec.min..=spec.max)
                                .text(spec.label)
                                .fixed_decimals(if spec.key == "scale" { 2 } else { 0 }),
                        )
                        .changed()
                    {
                        self.config.data["sizes"][spec.key] = json!(value);
                        changed = true;
                    }
                }
                ui.label("Настройки действуют только на серверные диалоги. Ширина 0 подбирается по содержимому.");
            }
            _ => {
                changed |= font_picker(
                    ui,
                    "dialog-font",
                    "Шрифт",
                    &mut self.config.data,
                    "fontFamily",
                    "fontName",
                    &self.config.font_names,
                    &mut self.font_filter,
                );
                for (key, label) in [
                    ("bold", "Жирный шрифт"),
                    ("preserveColors", "Сохранять исходные цвета текста"),
                    ("animations", "Сохранять анимации игры"),
                ] {
                    changed |= switch_value(ui, label, &mut self.config.data[key]);
                }
                egui::CollapsingHeader::new("Дополнительный CSS").show(ui, |ui| {
                    let mut css = self.config.data["customCss"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned();
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut css)
                                .desired_rows(10)
                                .desired_width(f32::INFINITY)
                                .code_editor(),
                        )
                        .changed()
                        && css.len() <= 262_144
                    {
                        self.config.data["customCss"] = json!(css);
                        changed = true;
                    }
                });
                if changed {
                    self.config.data["enabled"] = json!(true);
                }
            }
        }
        if changed {
            self.changed();
        }
    }

    fn fields(&mut self, ui: &mut egui::Ui, group: &str, keys: &[&str]) -> bool {
        let mut changed = false;
        for key in keys {
            if let Some(spec) = schema::WIDGETS
                .iter()
                .find(|spec| spec.group == group && spec.key == *key)
            {
                changed |= spec_control(ui, spec, &mut self.config, &mut self.font_filter);
            }
        }
        changed
    }

    fn widget_page(&mut self, ui: &mut egui::Ui, group: &str) {
        let mut changed = false;
        card(ui, "Основные настройки", |ui| match group {
            "radar" => {
                let previous = self.config.data["widgets"][group]["shape"]
                    .as_i64()
                    .unwrap_or(1);
                changed |= self.fields(ui, group, &["shape", "decorations", "effect"]);
                let shape = self.config.data["widgets"][group]["shape"]
                    .as_i64()
                    .unwrap_or(1);
                let width = self.config.data["widgets"][group]["width"]
                    .as_f64()
                    .unwrap_or(230.0);
                let height = self.config.data["widgets"][group]["height"]
                    .as_f64()
                    .unwrap_or(230.0);
                if previous != shape && shape >= 2 && (width - height).abs() < 1.0 {
                    self.config.data["widgets"][group]["width"] = json!(width.max(240.0));
                    self.config.data["widgets"][group]["height"] = json!(width.max(240.0) * 0.6);
                }
                if self.config.data["widgets"][group]["effect"]
                    .as_i64()
                    .unwrap_or(0)
                    > 0
                {
                    changed |= self.fields(ui, group, &["effectTarget"]);
                }
            }
            "safety" => {
                changed |= self.fields(ui, group, &["text", "font", "bold", "icon", "followRadar"]);
            }
            "chat" => {
                changed |= self.fields(ui, group, &["font", "bold", "lines"]);
            }
            "clock" => {
                changed |= self.fields(
                    ui,
                    group,
                    &[
                        "enabled",
                        "font",
                        "bold",
                        "shadow",
                        "seconds",
                        "milliseconds",
                        "format24",
                        "anchor",
                    ],
                );
                ui.label(
                    egui::RichText::new(
                        "Перетаскивание и колесо мыши доступны, пока открыто меню.",
                    )
                    .small()
                    .color(MUTED),
                );
            }
            "speedometer" => {
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in
                        [(1, "1. Цифровой"), (2, "2. Стрелочный"), (3, "3. Круглый")]
                    {
                        if ui
                            .selectable_label(
                                self.config.data["widgets"][group]["style"].as_i64() == Some(index),
                                name,
                            )
                            .clicked()
                        {
                            self.config.data["widgets"][group]["style"] = json!(index);
                            self.config.data["widgets"][group]["font"] =
                                json!(if index == 3 { 6 } else { 4 });
                            self.config.data["widgets"][group]["fontName"] =
                                json!(assets::font_name(if index == 3 { 6 } else { 4 }));
                            self.config.data["widgets"][group]["background"][3] =
                                json!(if index == 3 { 0.0 } else { 0.92 });
                            changed = true;
                        }
                    }
                });
                changed |= self.fields(ui, group, &["font", "fuel", "mileage", "indicators"]);
            }
            "gametext" => {
                changed |= self.fields(ui, group, &["font", "moneyFont", "bold", "preserveColors"]);
                ui.horizontal_wrapped(|ui| {
                    for (kind, label) in [
                        (0, "Показать пример сообщения"),
                        (1, "Показать пример изменения денег"),
                    ] {
                        if ui.button(label).clicked() {
                            self.config.data["widgets"][group]["enabled"] = json!(true);
                            self.preview_request = Some(kind);
                            changed = true;
                        }
                    }
                });
            }
            _ => {}
        });
        ui.add_space(7.0);
        let columns = if ui.available_width() > 700.0 { 2 } else { 1 };
        ui.columns(columns, |columns| {
            card(
                &mut columns[0],
                "Размеры и положение",
                |ui| {
                    for spec in schema::WIDGETS
                        .iter()
                        .filter(|spec| spec.group == group && spec.kind == Kind::Size)
                    {
                        let shape = self.config.data["widgets"][group]["shape"]
                            .as_i64()
                            .unwrap_or(0);
                        let effect = self.config.data["widgets"][group]["effect"]
                            .as_i64()
                            .unwrap_or(0);
                        if group == "radar" && spec.key == "effectPeriod" && effect == 0 {
                            continue;
                        }
                        let disabled = group == "radar"
                            && ((spec.key == "rounding" && [1, 2].contains(&shape))
                                || (spec.key == "height" && shape < 2))
                            || group == "safety"
                                && self.config.data["widgets"][group]["followRadar"]
                                    .as_bool()
                                    .unwrap_or(true)
                                && ["x", "bottom"].contains(&spec.key);
                        ui.add_enabled_ui(!disabled, |ui| {
                            changed |=
                                spec_control(ui, spec, &mut self.config, &mut self.font_filter);
                        });
                    }
                    if group == "radar" {
                        changed |= self.fields(ui, group, &["keepAspect"]);
                    }
                },
            );
            let index = usize::from(columns.len() > 1);
            card(&mut columns[index], "Цвета и обводка", |ui| {
                for spec in schema::WIDGETS
                    .iter()
                    .filter(|spec| spec.group == group && spec.kind == Kind::Color)
                {
                    if group == "radar"
                        && ["effectFirst", "effectSecond"].contains(&spec.key)
                        && self.config.data["widgets"][group]["effect"].as_i64() != Some(1)
                    {
                        continue;
                    }
                    changed |= spec_control(ui, spec, &mut self.config, &mut self.font_filter);
                }
            });
        });
        ui.add_space(8.0);
        if ui
            .button(if group == "clock" {
                "Сбросить часы"
            } else {
                "Вернуть оформление игры"
            })
            .clicked()
        {
            self.config.reset_widget(group);
            if group == "speedometer" {
                self.config.data["widgets"][group]["style"] = json!(0);
            }
            if group == "gametext" {
                self.config.data["widgets"][group]["enabled"] = json!(false);
            }
            self.changed();
            return;
        }
        if changed {
            if ["radar", "safety", "chat", "gametext"].contains(&group) {
                self.config.data["widgets"][group]["enabled"] = json!(true);
            }
            if group == "chat" {
                self.config.data["widgets"][group]["resize"] = json!(true);
                self.config.data["widgets"][group]["input"] = json!(true);
            }
            self.changed();
        }
    }

    fn player_hud_page(&mut self, ui: &mut egui::Ui, player: &PlayerHudState) {
        let mut changed = false;
        let mut restored = false;
        let previous = self.config.data["widgets"]["playerHud"]["style"]
            .as_i64()
            .unwrap_or(0);
        changed |= self.fields(ui, "playerHud", &["style"]);
        let selected = self.config.data["widgets"]["playerHud"]["style"]
            .as_i64()
            .unwrap_or(0);
        if selected != previous {
            let (width, weapon, money) = match selected {
                1 => (230, 76, 48),
                2 => (300, 90, 40),
                _ => (250, 80, 48),
            };
            for (key, value) in [
                ("width", width),
                ("weaponSize", weapon),
                ("moneySize", money),
            ] {
                self.config.data["widgets"]["playerHud"][key] = json!(value);
            }
        }
        if ui.button("Вернуть оригинальный HUD").clicked() {
            self.config.data["widgets"]["playerHud"]["style"] = json!(0);
            self.config.reset_positions("playerHud");
            changed = true;
            restored = true;
        }
        tabs(
            ui,
            &mut self.tabs[1],
            &["Расположение", "Полоски и проценты", "Оружие", "Деньги"],
        );
        match self.tabs[1] {
            0 => {
                changed |= self.fields(
                    ui,
                    "playerHud",
                    &["right", "top", "scale", "opacity", "width", "gap", "wanted"],
                );
            }
            1 => {
                egui::Grid::new("hud-bars")
                    .num_columns(5)
                    .striped(true)
                    .show(ui, |ui| {
                        for header in ["Порядок", "Показатель", "Показать", "Процент", "Цвет"]
                        {
                            ui.label(egui::RichText::new(header).small().color(MUTED));
                        }
                        ui.end_row();
                        for row in 0..4 {
                            let row_key = format!("row{row}");
                            let index = self.config.data["widgets"]["playerHud"][&row_key]
                                .as_u64()
                                .unwrap_or(row)
                                .min(3) as usize;
                            let base = ["health", "armour", "hunger", "air"][index];
                            ui.push_id(row, |ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_enabled(row > 0, egui::Button::new("Выше").small())
                                        .clicked()
                                    {
                                        let other = format!("row{}", row - 1);
                                        let value = self.config.data["widgets"]["playerHud"]
                                            [&other]
                                            .clone();
                                        self.config.data["widgets"]["playerHud"][&other] =
                                            json!(index);
                                        self.config.data["widgets"]["playerHud"][&row_key] = value;
                                        changed = true;
                                    }
                                    if ui
                                        .add_enabled(row < 3, egui::Button::new("Ниже").small())
                                        .clicked()
                                    {
                                        let other = format!("row{}", row + 1);
                                        let value = self.config.data["widgets"]["playerHud"]
                                            [&other]
                                            .clone();
                                        self.config.data["widgets"]["playerHud"][&other] =
                                            json!(index);
                                        self.config.data["widgets"]["playerHud"][&row_key] = value;
                                        changed = true;
                                    }
                                });
                                ui.label(["Здоровье", "Броня", "Голод", "Воздух в воде"][index]);
                                changed |= checkbox_value(
                                    ui,
                                    &mut self.config.data["widgets"]["playerHud"][base],
                                );
                                changed |= checkbox_value(
                                    ui,
                                    &mut self.config.data["widgets"]["playerHud"]
                                        [format!("{base}Value")],
                                );
                                changed |= color_control(
                                    ui,
                                    "",
                                    &mut self.config.data["widgets"]["playerHud"]
                                        [format!("{base}Color")],
                                );
                            });
                            ui.end_row();
                        }
                    });
                ui.label(
                    egui::RichText::new(
                        "Воздух показывается только в воде. Выше / Ниже меняют порядок полосок.",
                    )
                    .small()
                    .color(MUTED),
                );
                changed |= self.fields(
                    ui,
                    "playerHud",
                    &[
                        "valuePlacement",
                        "valueFont",
                        "barsY",
                        "barHeight",
                        "barGap",
                        "barOutline",
                        "barRounding",
                        "valueSize",
                        "valueOutline",
                        "valueColor",
                        "valueOutlineColor",
                        "barBackground",
                        "outlineColor",
                    ],
                );
                egui::CollapsingHeader::new("Свои иконки полосок").show(ui, |ui| {
                    changed |= self.fields(ui, "playerHud", &["statusIconSize", "statusIconGap"]);
                    self.image_editor(ui, 1, player);
                });
            }
            2 => {
                changed |= self.fields(
                    ui,
                    "playerHud",
                    &["weapon", "ammo", "weaponClip", "ammoFont"],
                );
                ui.label(format!(
                    "Текущее оружие: ID {}; патроны: {} / {}",
                    player.weapon, player.ammo, player.total_ammo
                ));
                changed |= self.fields(
                    ui,
                    "playerHud",
                    &[
                        "weaponSize",
                        "weaponPadding",
                        "frameWidth",
                        "frameRounding",
                        "weaponOutline",
                        "ammoSize",
                        "frameColor",
                        "weaponBackground",
                        "outlineColor",
                    ],
                );
                egui::CollapsingHeader::new("Своя картинка оружия / GIF")
                    .show(ui, |ui| self.image_editor(ui, 0, player));
            }
            _ => {
                changed |= self.fields(
                    ui,
                    "playerHud",
                    &[
                        "money",
                        "moneyZeros",
                        "moneyDots",
                        "currency",
                        "moneyFont",
                        "moneySize",
                        "moneyOutline",
                        "moneyGap",
                        "moneyColor",
                        "moneyOutlineColor",
                        "moneyEffect",
                    ],
                );
                if self.config.data["widgets"]["playerHud"]["moneyEffect"]
                    .as_i64()
                    .unwrap_or(0)
                    > 0
                {
                    changed |= self.fields(ui, "playerHud", &["moneyEffectPeriod"]);
                    if self.config.data["widgets"]["playerHud"]["moneyEffect"].as_i64() == Some(1) {
                        changed |= self.fields(
                            ui,
                            "playerHud",
                            &["moneyEffectFirst", "moneyEffectSecond"],
                        );
                    }
                }
            }
        }
        if changed {
            if !restored
                && self.config.data["widgets"]["playerHud"]["style"]
                    .as_i64()
                    .unwrap_or(0)
                    == 0
                && selected == previous
            {
                self.config.data["widgets"]["playerHud"]["style"] = json!(3);
            }
            self.changed();
        }
    }

    fn logo_page(&mut self, ui: &mut egui::Ui, player: &PlayerHudState) {
        let mut changed = self.fields(ui, "logo", &["style"]);
        tabs(
            ui,
            &mut self.tabs[2],
            &[
                "Надписи и сервер",
                "Расположение",
                "Цвета и анимация",
                "Своя картинка / GIF",
            ],
        );
        match self.tabs[2] {
            0 => {
                changed |= self.fields(ui, "logo", &["title", "subtitle", "emblem", "font"]);
                ui.label(format!("Текущий сервер: {}", player.server));
                changed |= self.fields(ui, "logo", &["server", "autoColor"]);
                ui.label("Для серверов 1-21 задана палитра. Для новых номеров цвет выбирается автоматически.");
            }
            1 => {
                changed |= self.fields(
                    ui,
                    "logo",
                    &[
                        "followHud",
                        "width",
                        "height",
                        "scale",
                        "gap",
                        "top",
                        "opacity",
                        "rounding",
                    ],
                );
                if !self.config.data["widgets"]["logo"]["followHud"]
                    .as_bool()
                    .unwrap_or(true)
                    || self.config.data["widgets"]["playerHud"]["style"]
                        .as_i64()
                        .unwrap_or(0)
                        == 0
                {
                    changed |= self.fields(ui, "logo", &["right"]);
                }
            }
            2 => {
                changed |= self.fields(ui, "logo", &["color"]);
                if !self.config.data["widgets"]["logo"]["autoColor"]
                    .as_bool()
                    .unwrap_or(true)
                {
                    changed |= self.fields(ui, "logo", &["accent"]);
                }
                changed |= self.fields(ui, "logo", &["background", "emblemColor", "effect"]);
                if self.config.data["widgets"]["logo"]["effect"]
                    .as_i64()
                    .unwrap_or(0)
                    > 0
                {
                    changed |= self.fields(ui, "logo", &["effectPeriod"]);
                    if self.config.data["widgets"]["logo"]["effect"].as_i64() == Some(1) {
                        changed |= self.fields(ui, "logo", &["effectFirst", "effectSecond"]);
                    }
                }
            }
            _ => {
                changed |= self.fields(ui, "logo", &["customMode"]);
                self.image_editor(ui, 2, player);
                ui.label(
                    "Для сервера без своей картинки используется общий логотип, затем CSS-вариант.",
                );
            }
        }
        if ui.button("Скрыть логотип").clicked() {
            self.config.data["widgets"]["logo"]["style"] = json!(0);
            changed = true;
        }
        if changed {
            self.changed();
        }
    }

    fn image_editor(&mut self, ui: &mut egui::Ui, kind: usize, player: &PlayerHudState) {
        ui.push_id(("image-editor", kind), |ui| {
            if kind == 2 {
                if self.config.data["widgets"]["logo"]["customMode"].as_i64() == Some(2) {
                    if self.image_ids[kind] < 1 {
                        self.image_ids[kind] = i64::from(player.server.max(1));
                    }
                    ui.horizontal(|ui| {
                        ui.label("Сервер картинки");
                        ui.add(egui::DragValue::new(&mut self.image_ids[kind]).range(1..=999_999));
                        if ui.button("Текущий сервер").clicked() {
                            self.image_ids[kind] = i64::from(player.server.max(1));
                        }
                    });
                } else {
                    self.image_ids[kind] = 0;
                }
            } else if kind == 1 {
                egui::ComboBox::from_id_salt("bar-id")
                    .selected_text(
                        ["Здоровье", "Броня", "Голод", "Воздух"]
                            [self.image_ids[kind].clamp(0, 3) as usize],
                    )
                    .show_ui(ui, |ui| {
                        for (id, label) in
                            ["Здоровье", "Броня", "Голод", "Воздух"].iter().enumerate()
                        {
                            ui.selectable_value(&mut self.image_ids[kind], id as i64, *label);
                        }
                    });
                let key = [
                    "healthIconSide",
                    "armourIconSide",
                    "hungerIconSide",
                    "airIconSide",
                ][self.image_ids[kind].clamp(0, 3) as usize];
                if self.fields(ui, "playerHud", &[key]) {
                    self.changed();
                }
            } else {
                ui.horizontal(|ui| {
                    ui.label("ID оружия");
                    ui.add(egui::DragValue::new(&mut self.image_ids[kind]).range(0..=65_535));
                    if ui.button("Текущее").clicked() {
                        self.image_ids[kind] = i64::from(player.weapon.max(0));
                    }
                });
            }
            let id = self.image_ids[kind];
            if self.image_loaded_ids[kind] != id {
                self.image_inputs[kind] = self.config.data[image_map(kind)][id.to_string()]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                self.image_loaded_ids[kind] = id;
            }
            ui.label(
                egui::RichText::new(
                    "PNG, JPG, BMP, GIF. Вставьте Base64 или data:image/...;base64,...",
                )
                .small()
                .color(MUTED),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.image_inputs[kind])
                    .desired_width(f32::INFINITY)
                    .desired_rows(5),
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        self.image_receiver.is_none(),
                        egui::Button::new("Применить картинку"),
                    )
                    .clicked()
                {
                    let source = self.image_inputs[kind].trim().to_owned();
                    if source.is_empty() || source.len() > 8_388_608 {
                        self.notify("Вставьте Base64 размером до 8 МБ");
                    } else {
                        let (tx, rx) = mpsc::channel();
                        self.image_receiver = Some(rx);
                        std::thread::spawn(move || {
                            let result = validate_image(kind, id, source);
                            let _ = tx.send(result);
                        });
                    }
                }
                if ui.button("Удалить замену").clicked() {
                    if let Some(images) = self.config.data[image_map(kind)].as_object_mut() {
                        images.remove(&id.to_string());
                    }
                    self.image_inputs[kind].clear();
                    self.image_previews.remove(&(kind, id));
                    self.changed();
                }
            });
            if self.image_receiver.is_some() {
                ui.label(
                    egui::RichText::new("Чтение кадров картинки...")
                        .small()
                        .color(MUTED),
                );
            }
            if let Some(texture) = self.image_previews.get(&(kind, id)) {
                let size = texture.size_vec2();
                ui.image((texture.id(), size * (140.0 / size.x.max(size.y)).min(1.0)));
            }
        });
    }

    fn layout_page(&mut self, ui: &mut egui::Ui) {
        ui.label("Пока открыто меню, перетаскивайте любой элемент интерфейса левой кнопкой мыши. Колесо меняет его размер.");
        let selected = self
            .elements
            .iter()
            .find(|element| element.id == self.selected_element)
            .map(|element| element.label.as_str())
            .unwrap_or(&self.selected_element);
        egui::ComboBox::from_id_salt("layout-element")
            .selected_text(if selected.is_empty() {
                "Выберите на экране"
            } else {
                selected
            })
            .show_ui(ui, |ui| {
                for element in &self.elements {
                    ui.selectable_value(
                        &mut self.selected_element,
                        element.id.clone(),
                        &element.label,
                    );
                }
            });
        if !self.selected_element.is_empty() {
            let layout = self.config.data["layouts"]
                .as_object_mut()
                .unwrap()
                .entry(self.selected_element.clone())
                .or_insert_with(|| json!({"x":0.0,"y":0.0,"scale":1.0}));
            let mut x = layout["x"].as_f64().unwrap_or(0.0);
            let mut y = layout["y"].as_f64().unwrap_or(0.0);
            let mut scale = layout["scale"].as_f64().unwrap_or(1.0);
            let mut changed = ui
                .add(
                    egui::Slider::new(&mut x, -10_000.0..=10_000.0).text("Смещение по горизонтали"),
                )
                .changed();
            changed |= ui
                .add(egui::Slider::new(&mut y, -10_000.0..=10_000.0).text("Смещение по вертикали"))
                .changed();
            changed |= ui
                .add(
                    egui::Slider::new(&mut scale, 0.25..=4.0)
                        .text("Масштаб элемента")
                        .fixed_decimals(2),
                )
                .changed();
            if ui.button("Сбросить выбранный элемент").clicked() {
                x = 0.0;
                y = 0.0;
                scale = 1.0;
                changed = true;
            }
            if changed {
                *layout = json!({"x":x,"y":y,"scale":scale});
                self.changed();
            }
        }
        if ui.button("Сбросить расположение всех элементов").clicked()
        {
            self.config.data["layouts"] = json!({});
            self.changed();
        }
    }

    fn profiles_page(&mut self, ui: &mut egui::Ui, status: &BridgeStatus) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Сохранить профиль").clicked() {
                match self.config.save_profile(&self.root) {
                    Ok(()) => self.notify("Профиль сохранён"),
                    Err(error) => self.notify(error),
                }
            }
            if ui.button("Загрузить профиль").clicked() {
                match self.config.load_profile(&self.root) {
                    Ok(()) => {
                        self.changed();
                        self.notify("Профиль загружен");
                    }
                    Err(error) => self.notify(error),
                }
            }
        });
        ui.label(
            "Изменения сохраняются автоматически в settings.ini. Отдельный профиль: profile.ini.",
        );
        if ui.button("Перечитать настройки с диска").clicked() {
            match Config::load(&self.root) {
                Ok(mut config) => {
                    config.set_font_names(&assets::font_names());
                    self.config = config;
                    self.changed();
                    self.refresh_requested = true;
                    self.notify("Настройки перечитаны");
                }
                Err(error) => self.notify(error),
            }
        }
        egui::CollapsingHeader::new("Список диалогов").show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Поиск диалога"));
            let search = self.search.to_lowercase();
            egui::Grid::new("interfaces")
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    for title in ["Диалог", "Открыт", "Загружен", "Тип"] {
                        ui.label(egui::RichText::new(title).color(MUTED));
                    }
                    ui.end_row();
                    for view in &status.views {
                        for interface in &view.interfaces {
                            if !interface.name.to_lowercase().contains(&search)
                                && !interface.title.to_lowercase().contains(&search)
                            {
                                continue;
                            }
                            ui.label(format!("{}: {}", interface.name, interface.title));
                            ui.label(if interface.opened { "Да" } else { "-" });
                            ui.label(if interface.loaded { "Да" } else { "-" });
                            ui.label(&interface.kind);
                            ui.end_row();
                        }
                    }
                });
        });
    }

    fn edit_layout(&mut self, ctx: &egui::Context) {
        let (pointer, down, pressed, delta, wheel) = ctx.input(|input| {
            (
                input.pointer.hover_pos(),
                input.pointer.primary_down(),
                input.pointer.primary_pressed(),
                input.pointer.delta(),
                input.raw_scroll_delta.y,
            )
        });
        if !down && !self.dragged_element.is_empty() {
            self.dragged_element.clear();
            self.save_due = Some(Instant::now());
        }
        let Some(pointer) = pointer else { return };
        let candidate = if !self.dragged_element.is_empty() {
            self.elements
                .iter()
                .find(|element| element.id == self.dragged_element)
        } else if self.menu_rect.is_none_or(|rect| !rect.contains(pointer))
            && !ctx.is_pointer_over_area()
        {
            self.elements
                .iter()
                .filter(|element| {
                    element.width > 2.0
                        && element.height > 2.0
                        && Rect::from_min_size(
                            pos2(element.x, element.y),
                            vec2(element.width, element.height),
                        )
                        .contains(pointer)
                })
                .min_by(|left, right| {
                    (left.width * left.height).total_cmp(&(right.width * right.height))
                })
        } else {
            None
        };
        let Some(element) = candidate.cloned() else {
            return;
        };
        ctx.set_cursor_icon(egui::CursorIcon::Move);
        if pressed {
            self.dragged_element = element.id.clone();
            self.selected_element = element.id.clone();
        }
        let mut changed = false;
        let layout = self.config.data["layouts"]
            .as_object_mut()
            .unwrap()
            .entry(element.id.clone())
            .or_insert_with(|| json!({"x":0.0,"y":0.0,"scale":1.0}));
        if !pressed && !self.dragged_element.is_empty() && delta != Vec2::ZERO {
            let divisor = element.parent_scale.max(0.01);
            layout["x"] = json!(
                (layout["x"].as_f64().unwrap_or(0.0) + f64::from(delta.x / divisor))
                    .clamp(-10_000.0, 10_000.0)
            );
            layout["y"] = json!(
                (layout["y"].as_f64().unwrap_or(0.0) + f64::from(delta.y / divisor))
                    .clamp(-10_000.0, 10_000.0)
            );
            changed = true;
        }
        if wheel != 0.0 {
            layout["scale"] = json!(
                (layout["scale"].as_f64().unwrap_or(1.0) * 1.08_f64.powf(f64::from(wheel / 40.0)))
                    .clamp(0.25, 4.0)
            );
            self.selected_element = element.id.clone();
            changed = true;
        }
        let scale = layout["scale"].as_f64().unwrap_or(1.0);
        if changed {
            self.changed();
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            Id::new("layout-highlight"),
        ));
        painter.rect_stroke(
            Rect::from_min_size(
                pos2(element.x, element.y),
                vec2(element.width, element.height),
            ),
            4.0,
            Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(139, 132, 255, 235)),
            egui::StrokeKind::Outside,
        );
        egui::Area::new(Id::new("layout-tooltip"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pointer + vec2(15.0, 20.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(&element.label);
                    ui.label(
                        egui::RichText::new(format!(
                            "ЛКМ: перемещение | Колесо: масштаб {:.0}%",
                            scale * 100.0
                        ))
                        .small()
                        .color(MUTED),
                    );
                });
            });
    }
}

fn draw_software_cursor(ctx: &egui::Context) {
    #[repr(C)]
    struct CursorInfo {
        size: u32,
        flags: u32,
        cursor: isize,
        x: i32,
        y: i32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorInfo(info: *mut CursorInfo) -> i32;
    }
    let mut info = CursorInfo {
        size: std::mem::size_of::<CursorInfo>() as u32,
        flags: 0,
        cursor: 0,
        x: 0,
        y: 0,
    };
    if unsafe { GetCursorInfo(&mut info) } == 0 || info.flags & 1 != 0 {
        return;
    }
    let Some(position) = ctx.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::new("oldDialogs-software-cursor"),
    ));
    if ctx.output(|output| output.cursor_icon) == egui::CursorIcon::Text {
        let top = position + vec2(0.0, -8.0);
        let bottom = position + vec2(0.0, 9.0);
        for (start, end) in [
            (top, bottom),
            (top - vec2(4.0, 0.0), top + vec2(4.0, 0.0)),
            (bottom - vec2(4.0, 0.0), bottom + vec2(4.0, 0.0)),
        ] {
            painter.line_segment([start, end], Stroke::new(3.0_f32, Color32::BLACK));
            painter.line_segment([start, end], Stroke::new(1.0_f32, Color32::WHITE));
        }
    } else {
        let points: Vec<Pos2> = [
            vec2(0.0, 0.0),
            vec2(0.0, 18.0),
            vec2(4.7, 14.0),
            vec2(8.5, 22.0),
            vec2(11.8, 20.5),
            vec2(8.0, 12.6),
            vec2(15.5, 12.6),
        ]
        .into_iter()
        .map(|point| position + point)
        .collect();
        let mut mesh = egui::Mesh::default();
        for point in &points {
            mesh.colored_vertex(*point, Color32::WHITE);
        }
        mesh.indices
            .extend_from_slice(&[0, 1, 2, 0, 2, 5, 0, 5, 6, 2, 3, 4, 2, 4, 5]);
        painter.add(mesh);
        painter.add(egui::Shape::closed_line(
            points,
            Stroke::new(1.5_f32, Color32::BLACK),
        ));
    }
}

fn image_map(kind: usize) -> &'static str {
    ["weaponImages", "statusIcons", "logoImages"][kind.min(2)]
}

fn validate_image(kind: usize, id: i64, source: String) -> Result<ImageResult, String> {
    use image::AnimationDecoder;
    use std::io::Cursor;
    let encoded = if source.starts_with("data:") {
        let (header, data) = source
            .split_once(',')
            .ok_or("Нужен Base64 или data:image/...;base64,...")?;
        if !header.contains(";base64") {
            return Err("Нужен Base64 или data:image/...;base64,...".into());
        }
        data
    } else {
        &source
    };
    let encoded: String = encoded
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Некорректный Base64".to_owned())?;
    let format = image::guess_format(&bytes)
        .map_err(|_| "Картинка не распознана. Поддерживаются PNG, JPG, BMP и GIF".to_owned())?;
    if ![
        image::ImageFormat::Png,
        image::ImageFormat::Jpeg,
        image::ImageFormat::Bmp,
        image::ImageFormat::Gif,
    ]
    .contains(&format)
    {
        return Err("Поддерживаются PNG, JPG, BMP и GIF".into());
    }
    let dimensions = image::ImageReader::with_format(Cursor::new(&bytes), format)
        .into_dimensions()
        .map_err(|error| error.to_string())?;
    if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 2048 || dimensions.1 > 2048 {
        return Err("Максимальный размер картинки: 2048 x 2048".into());
    }
    let rgba = if format == image::ImageFormat::Gif {
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&bytes))
            .map_err(|error| error.to_string())?;
        let mut first = None;
        let mut total = 0usize;
        let mut count = 0usize;
        for frame in decoder.into_frames() {
            let frame = frame.map_err(|error| error.to_string())?;
            count += 1;
            if count > 240 {
                return Err("GIF должен содержать не более 240 кадров".into());
            }
            total += frame.buffer().len();
            if total > 100_663_296 {
                return Err("GIF после распаковки превышает 96 МБ".into());
            }
            if first.is_none() {
                first = Some(frame.into_buffer());
            }
        }
        first.ok_or("GIF не содержит кадров")?
    } else {
        image::load_from_memory_with_format(&bytes, format)
            .map_err(|error| error.to_string())?
            .to_rgba8()
    };
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        &rgba,
    );
    Ok(ImageResult {
        kind,
        id,
        source,
        image,
    })
}

fn dialog_preview(ui: &mut egui::Ui, config: &Value) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().max(180.0), 245.0), Sense::hover());
    let p = ui.painter();
    let colors = &config["colors"];
    let width = rect.width();
    let dialog = Rect::from_min_size(rect.min + vec2(12.0, 16.0), vec2(width - 24.0, 214.0));
    p.rect_filled(dialog, 0.0, rgba(&colors["body"]));
    p.rect_stroke(
        dialog,
        0.0,
        Stroke::new(
            config["sizes"]["borderWidth"].as_f64().unwrap_or(1.0) as f32,
            rgba(&colors["border"]),
        ),
        egui::StrokeKind::Inside,
    );
    p.rect_filled(
        Rect::from_min_size(
            dialog.min + vec2(1.0, 1.0),
            vec2(dialog.width() - 2.0, 27.0),
        ),
        0.0,
        rgba(&colors["header"]),
    );
    p.text(
        dialog.min + vec2(12.0, 6.0),
        Align2::LEFT_TOP,
        "Пример диалога",
        FontId::proportional(16.0),
        rgba(&colors["title"]),
    );
    for (index, label) in [
        "Выберите действие",
        "Информация о персонаже",
        "Настройки игры",
        "Список транспорта",
    ]
    .iter()
    .enumerate()
    {
        let y = 40.0 + index as f32 * 27.0;
        if index == 1 {
            p.rect_filled(
                Rect::from_min_size(
                    dialog.min + vec2(10.0, y - 3.0),
                    vec2(dialog.width() - 20.0, 25.0),
                ),
                0.0,
                rgba(&colors["accent"]),
            );
        }
        p.text(
            dialog.min + vec2(15.0, y),
            Align2::LEFT_TOP,
            *label,
            FontId::proportional(16.0),
            rgba(&colors[if index == 0 { "text" } else { "listText" }]),
        );
    }
    for (index, title) in ["Выбрать", "Отмена"].iter().enumerate() {
        let button = Rect::from_min_size(
            pos2(
                dialog.center().x - 108.0 + index as f32 * 112.0,
                dialog.bottom() - 38.0,
            ),
            vec2(105.0, 25.0),
        );
        p.rect_filled(button, 0.0, rgba(&colors["button"]));
        p.rect_stroke(
            button,
            0.0,
            Stroke::new(1.0_f32, rgba(&colors["border"])),
            egui::StrokeKind::Inside,
        );
        p.text(
            button.center(),
            Align2::CENTER_CENTER,
            *title,
            FontId::proportional(16.0),
            rgba(&colors["buttonText"]),
        );
    }
}

fn navigation_icon(painter: &egui::Painter, center: Pos2, index: usize, color: Color32) {
    let stroke = Stroke::new(1.4_f32, color);
    if [1, 4, 7].contains(&index) {
        painter.circle_stroke(center, 7.0, stroke);
        if index == 4 {
            painter.line_segment([center, center + vec2(0.0, -4.0)], stroke);
            painter.line_segment([center, center + vec2(4.0, 2.0)], stroke);
        } else {
            painter.line_segment([center + vec2(-9.0, 0.0), center + vec2(9.0, 0.0)], stroke);
            painter.line_segment([center + vec2(0.0, -9.0), center + vec2(0.0, 9.0)], stroke);
        }
    } else if index == 2 {
        painter.add(egui::Shape::closed_line(
            [
                vec2(0.0, -8.0),
                vec2(7.0, -5.0),
                vec2(6.0, 3.0),
                vec2(0.0, 8.0),
                vec2(-6.0, 3.0),
                vec2(-7.0, -5.0),
            ]
            .iter()
            .map(|point| center + *point)
            .collect(),
            stroke,
        ));
    } else if index == 9 {
        painter.rect_stroke(
            Rect::from_min_max(center + vec2(-7.0, -7.0), center + vec2(1.0, 1.0)),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            Rect::from_min_max(center + vec2(-1.0, -1.0), center + vec2(7.0, 7.0)),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    } else {
        painter.rect_stroke(
            Rect::from_center_size(center, vec2(14.0, 14.0)),
            2.0,
            stroke,
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [center + vec2(-4.0, -2.0), center + vec2(4.0, -2.0)],
            stroke,
        );
        painter.line_segment([center + vec2(-4.0, 3.0), center + vec2(2.0, 3.0)], stroke);
    }
}
