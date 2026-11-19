use super::{INK, MUTED};
use crate::schema::{Kind, Spec};
use crate::settings::Config;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

pub(super) fn tabs(ui: &mut egui::Ui, selected: &mut usize, titles: &[&str]) {
    ui.add_space(7.0);
    ui.horizontal_wrapped(|ui| {
        for (index, title) in titles.iter().enumerate() {
            ui.selectable_value(selected, index, *title);
        }
    });
    ui.separator();
    ui.add_space(5.0);
}

pub(super) fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(14, 15, 19, 140))
        .stroke(Stroke::new(
            1.0_f32,
            Color32::from_rgba_unmultiplied(48, 51, 64, 140),
        ))
        .corner_radius(9)
        .inner_margin(egui::Margin::symmetric(14, 13))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().max(120.0));
            ui.label(egui::RichText::new(title).small().color(MUTED));
            ui.add_space(4.0);
            add(ui);
        });
}

pub(super) fn checkbox_value(ui: &mut egui::Ui, value: &mut Value) -> bool {
    let mut flag = value.as_bool().unwrap_or(false);
    let changed = ui.checkbox(&mut flag, "").changed();
    if changed {
        *value = json!(flag);
    }
    changed
}

pub(super) fn switch_value(ui: &mut egui::Ui, label: &str, value: &mut Value) -> bool {
    let mut flag = value.as_bool().unwrap_or(false);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width().max(100.0), 30.0), Sense::click());
    if response.clicked() {
        flag = !flag;
        *value = json!(flag);
    }
    let track_height = rect.height() * 0.56;
    let track_width = track_height * 1.9;
    let track = Rect::from_min_size(
        pos2(
            rect.right() - track_width,
            rect.center().y - track_height * 0.5,
        ),
        vec2(track_width, track_height),
    );
    ui.painter().text(
        rect.min + vec2(0.0, 7.0),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(16.0),
        if response.hovered() {
            Color32::from_rgb(246, 244, 255)
        } else {
            INK
        },
    );
    ui.painter().rect_filled(
        track,
        track_height * 0.5,
        if flag {
            Color32::from_rgb(119, 108, 246)
        } else {
            Color32::from_rgb(49, 51, 64)
        },
    );
    ui.painter().circle_filled(
        pos2(
            if flag {
                track.right() - track_height * 0.5
            } else {
                track.left() + track_height * 0.5
            },
            track.center().y,
        ),
        track_height * 0.5 - 3.0,
        if flag {
            Color32::from_rgb(245, 244, 255)
        } else {
            Color32::from_rgb(139, 142, 157)
        },
    );
    response.clicked()
}

pub(super) fn color_control(ui: &mut egui::Ui, label: &str, value: &mut Value) -> bool {
    let mut channels = [0.0, 0.0, 0.0, 1.0];
    for (index, channel) in channels.iter_mut().enumerate() {
        *channel = value[index]
            .as_f64()
            .unwrap_or(if index == 3 { 1.0 } else { 0.0 }) as f32;
    }
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = ui
            .color_edit_button_rgba_unmultiplied(&mut channels)
            .changed();
        if !label.is_empty() {
            ui.label(label);
        }
    });
    if changed {
        *value = json!(channels);
    }
    changed
}

pub(super) fn font_picker(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    label: &str,
    data: &mut Value,
    key: &str,
    name_key: &str,
    names: &[String],
    filter: &mut String,
) -> bool {
    let mut index = data[key].as_u64().unwrap_or(0) as usize;
    index = index.min(names.len().saturating_sub(1));
    let previous = index;
    ui.label(label);
    egui::ComboBox::from_id_salt(id)
        .width(ui.available_width().max(150.0))
        .selected_text(names.get(index).map(String::as_str).unwrap_or("Arial"))
        .show_ui(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(filter)
                    .hint_text("Поиск шрифта")
                    .desired_width(f32::INFINITY),
            );
            let needle = filter.to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (value, name) in names.iter().enumerate() {
                        if needle.is_empty() || name.to_lowercase().contains(&needle) {
                            ui.selectable_value(&mut index, value, name);
                        }
                    }
                });
            ui.label(
                egui::RichText::new(format!("Доступно шрифтов: {}", names.len()))
                    .small()
                    .color(MUTED),
            );
        });
    if index != previous {
        data[key] = json!(index);
        data[name_key] = json!(names[index]);
        true
    } else {
        false
    }
}

pub(super) fn spec_control(
    ui: &mut egui::Ui,
    spec: &Spec,
    config: &mut Config,
    filter: &mut String,
) -> bool {
    let names = &config.font_names;
    let widget = &mut config.data["widgets"][spec.group];
    ui.push_id((spec.group, spec.key), |ui| match spec.kind {
        Kind::Flag => {
            let combo = match (spec.group, spec.key) {
                ("safety", "followRadar") => Some(("Положение", ["Над радаром", "Свободное"])),
                ("logo", "followHud") => Some(("Привязка", ["Над HUD", "Свободное положение"])),
                ("logo", "autoColor") => Some(("Цвет сервера", ["Автоматический", "Свой цвет"])),
                _ => None,
            };
            if let Some((label, options)) = combo {
                let mut index = usize::from(!widget[spec.key].as_bool().unwrap_or(true));
                let previous = index;
                ui.label(label);
                egui::ComboBox::from_id_salt(spec.key)
                    .width(ui.available_width())
                    .selected_text(options[index])
                    .show_ui(ui, |ui| {
                        for (value, title) in options.iter().enumerate() {
                            ui.selectable_value(&mut index, value, *title);
                        }
                    });
                if previous != index {
                    widget[spec.key] = json!(index == 0);
                    true
                } else {
                    false
                }
            } else {
                let label = match (spec.group, spec.key) {
                    ("radar", "decorations") => "Декоративная рамка игры",
                    ("safety", "icon") => "Значок щита",
                    ("clock", "seconds") => "Секунды",
                    ("clock", "milliseconds") => "Миллисекунды",
                    ("clock", "format24") => "24-часовой формат времени",
                    ("speedometer", "indicators") => "Индикаторы",
                    _ => spec.label,
                };
                switch_value(ui, label, &mut widget[spec.key])
            }
        }
        Kind::Color => color_control(ui, spec.label, &mut widget[spec.key]),
        Kind::Text => {
            ui.label(spec.label);
            let mut value = widget[spec.key].as_str().unwrap_or("").to_owned();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut value)
                        .desired_width(f32::INFINITY)
                        .char_limit(spec.max as usize),
                )
                .changed()
            {
                widget[spec.key] = json!(value);
                true
            } else {
                false
            }
        }
        Kind::Size => {
            ui.label(spec.label);
            let mut value = widget[spec.key].as_f64().unwrap_or(spec.min);
            if ui
                .add(
                    egui::Slider::new(&mut value, spec.min..=spec.max)
                        .fixed_decimals(if spec.max <= 3.0 { 2 } else { 1 }),
                )
                .changed()
            {
                widget[spec.key] = json!(value);
                true
            } else {
                false
            }
        }
        Kind::Choice if spec.max < 0.0 => font_picker(
            ui,
            spec.key,
            spec.label,
            widget,
            spec.key,
            &format!("{}Name", spec.key),
            names,
            filter,
        ),
        Kind::Choice => {
            let mut value = widget[spec.key].as_i64().unwrap_or(0);
            let previous = value;
            if spec.options.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(spec.label);
                    ui.add(
                        egui::DragValue::new(&mut value).range(spec.min as i64..=spec.max as i64),
                    );
                });
            } else {
                ui.label(spec.label);
                egui::ComboBox::from_id_salt(spec.key)
                    .width(ui.available_width())
                    .selected_text(
                        spec.options
                            .get(value.max(0) as usize)
                            .copied()
                            .unwrap_or(spec.options[0]),
                    )
                    .show_ui(ui, |ui| {
                        for (index, title) in spec.options.iter().enumerate() {
                            ui.selectable_value(&mut value, index as i64, *title);
                        }
                    });
            }
            if value != previous {
                widget[spec.key] = json!(value);
                true
            } else {
                false
            }
        }
    })
    .inner
}

pub(super) fn rgba(value: &Value) -> Color32 {
    let channel = |index: usize| {
        (value[index]
            .as_f64()
            .unwrap_or(if index == 3 { 1.0 } else { 0.0 })
            .clamp(0.0, 1.0)
            * 255.0)
            .round() as u8
    };
    Color32::from_rgba_unmultiplied(channel(0), channel(1), channel(2), channel(3))
}
