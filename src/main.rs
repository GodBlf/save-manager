#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod theme;

use eframe::egui::{self, Align, Layout, RichText, Vec2};
use save_manager::{GameInput, Manager, Recovery, Result, Snapshot, platform};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};
use uuid::Uuid;

fn main() -> eframe::Result {
    let manager = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "找不到 Windows 应用数据目录".to_owned())
        .and_then(|path| {
            Manager::open(&path.join("SaveManager")).map_err(|error| error.to_string())
        });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040., 690.])
            .with_min_inner_size([780., 510.]),
        ..Default::default()
    };
    eframe::run_native(
        "存档管理器",
        options,
        Box::new(move |creation| {
            configure_fonts(&creation.egui_ctx);
            let appearance = manager.as_ref().map_or(Ok(None), Manager::appearance);
            theme::configure(
                &creation.egui_ctx,
                appearance.as_ref().copied().unwrap_or_default(),
            );
            let mut app = App::new(manager);
            if let Err(error) = appearance {
                app.report(Err(error), "");
            }
            Ok(Box::new(app))
        }),
    )
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windows = std::env::var_os("WINDIR").map(PathBuf::from);
    let font = windows.as_ref().and_then(|dir| {
        ["msyh.ttc", "msjh.ttc", "simsun.ttc"]
            .iter()
            .find_map(|name| std::fs::read(dir.join("Fonts").join(name)).ok())
    });
    if let Some(font) = font {
        fonts
            .font_data
            .insert("cjk".into(), egui::FontData::from_owned(font).into());
        for family in fonts.families.values_mut() {
            family.push("cjk".into());
        }
    }
    ctx.set_fonts(fonts);
}

type JobResult = (Manager, Result<()>);

struct App {
    manager: Option<Manager>,
    job: Option<(Action, Receiver<JobResult>)>,
    failure: Option<String>,
    selected: Option<Uuid>,
    snapshots: Vec<Snapshot>,
    recovery: Option<Recovery>,
    editor: Option<GameEditor>,
    dialog: Option<Dialog>,
    name: String,
    status: Option<(String, bool)>,
}

struct GameEditor {
    id: Option<Uuid>,
    name: String,
    root: String,
    paths: Vec<String>,
}

enum Dialog {
    Rename(Snapshot, String),
    Delete(Snapshot),
    Restore(Snapshot, Vec<PathBuf>),
    Remove,
    Recover(Recovery),
}

#[derive(Clone, Copy)]
enum Action {
    Create,
    Delete,
    Restore,
    Recover,
}

impl Action {
    fn success(self) -> &'static str {
        match self {
            Self::Create => "快照已创建",
            Self::Delete => "快照已移到回收站",
            Self::Restore => "存档已恢复",
            Self::Recover => "恢复记录已处理",
        }
    }
}

impl App {
    fn new(manager: std::result::Result<Manager, String>) -> Self {
        let failure = manager.as_ref().err().cloned();
        let mut app = Self {
            manager: manager.ok(),
            job: None,
            failure,
            selected: None,
            snapshots: Vec::new(),
            recovery: None,
            editor: None,
            dialog: None,
            name: String::new(),
            status: None,
        };
        if let Some(manager) = &app.manager {
            app.selected = manager
                .games()
                .iter()
                .find(|game| manager.has_pending_recovery(game.id).unwrap_or(true))
                .or_else(|| manager.games().first())
                .map(|game| game.id);
            if !manager.notices().is_empty() {
                app.status = Some((manager.notices().join("\n"), true));
            }
        }
        app.refresh();
        if let Some(recovery) = &app.recovery {
            app.dialog = Some(Dialog::Recover(recovery.clone()));
        }
        app
    }

    fn manager(&self) -> &Manager {
        self.manager.as_ref().expect("only available when idle")
    }

    fn refresh(&mut self) {
        self.snapshots.clear();
        self.recovery = None;
        if let (Some(manager), Some(id)) = (&self.manager, self.selected) {
            match manager.snapshots(id) {
                Ok(snapshots) => self.snapshots = snapshots,
                Err(error) => self.status = Some((error.to_string(), true)),
            }
            match manager.pending_recovery(id) {
                Ok(recovery) => self.recovery = recovery,
                Err(error) => self.status = Some((error.to_string(), true)),
            }
        }
    }

    fn report(&mut self, result: Result<()>, success: &str) {
        self.status = Some(match result {
            Ok(()) => (success.to_owned(), false),
            Err(error) => (error.to_string(), true),
        });
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        kind: Action,
        action: impl FnOnce(&mut Manager) -> Result<()> + Send + 'static,
    ) {
        let mut manager = self.manager.take().expect("only one operation");
        let (sender, receiver) = mpsc::channel();
        let context = ctx.clone();
        std::thread::spawn(move || {
            let result = action(&mut manager);
            let _ = sender.send((manager, result));
            context.request_repaint();
        });
        self.job = Some((kind, receiver));
    }

    fn add_game(&mut self) {
        self.editor = Some(GameEditor {
            id: None,
            name: String::new(),
            root: String::new(),
            paths: Vec::new(),
        });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("games")
            .exact_width(216.)
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(14),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("游戏");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("＋ 添加游戏").clicked() {
                            self.add_game();
                        }
                    });
                });
                let games = self.manager().games().to_vec();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for game in games {
                        let needs_recovery =
                            self.manager().has_pending_recovery(game.id).unwrap_or(true);
                        let label = if needs_recovery {
                            format!("{} · 待处理恢复", game.input.name)
                        } else {
                            game.input.name
                        };
                        let selected = self.selected == Some(game.id);
                        if ui
                            .add_sized(
                                [ui.available_width(), 36.],
                                egui::Button::selectable(selected, &label)
                                    .frame_when_inactive(selected)
                                    .truncate(),
                            )
                            .on_hover_text(label)
                            .clicked()
                        {
                            self.selected = Some(game.id);
                            self.name.clear();
                            self.refresh();
                            if let Some(recovery) = &self.recovery {
                                self.dialog = Some(Dialog::Recover(recovery.clone()));
                            }
                        }
                    }
                });
            });
    }

    fn workspace(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(22))
            .show(ctx, |ui| {
                let Some(id) = self.selected else {
                    theme::card(ui).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.heading("还没有游戏");
                        ui.weak("添加游戏后，选择要追踪的存档文件和目录。");
                        if theme::primary(ui, "添加游戏", true).clicked() {
                            self.add_game();
                        }
                    });
                    return;
                };
                let Ok(game) = self.manager().game(id).cloned() else {
                    return;
                };
                let input = game.input;
                ui.horizontal(|ui| {
                    let title_width = (ui.available_width() - 185.).max(60.);
                    ui.allocate_ui_with_layout(
                        Vec2::new(title_width, 36.),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&input.name).size(27.).strong())
                                    .truncate(),
                            )
                            .on_hover_text(&input.name);
                        },
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(self.recovery.is_none(), |ui| {
                            if ui.button("移除游戏").clicked() {
                                self.dialog = Some(Dialog::Remove);
                            }
                            if ui.button("编辑配置").clicked() {
                                self.editor = Some(GameEditor {
                                    id: Some(id),
                                    name: input.name.clone(),
                                    root: input.root.display().to_string(),
                                    paths: input
                                        .paths
                                        .iter()
                                        .map(|path| path.display().to_string())
                                        .collect(),
                                });
                            }
                        });
                    });
                });
                if let Some(recovery) = &self.recovery {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(ui.visuals().error_fg_color, "此游戏有未处理的恢复记录。");
                        if ui.button("处理恢复").clicked() {
                            self.dialog = Some(Dialog::Recover(recovery.clone()));
                        }
                    });
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        theme::card(ui).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.paths(ui, &input);
                        });
                        ui.add_space(8.);
                        theme::card(ui).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            self.history(ui, ctx, id);
                        });
                    });
            });
    }

    fn history(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, id: Uuid) {
        ui.horizontal(|ui| {
            ui.heading("快照历史");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("刷新").clicked() {
                    self.refresh();
                }
            });
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.name)
                    .hint_text("快照名称（可选）")
                    .desired_width(260.),
            );
            if theme::primary(ui, "创建快照", self.recovery.is_none()).clicked() {
                let name = self.name.clone();
                self.start(ctx, Action::Create, move |manager| {
                    manager.create_snapshot(id, &name)
                });
            }
        });
        if self.job.is_some() {
            return;
        }
        if self.snapshots.is_empty() {
            ui.weak("当前目录还没有快照。");
            return;
        }
        ui.add_space(3.);
        history_row(ui, "名称", "时间", false, |ui| {
            ui.weak("操作");
        });
        for snapshot in self.snapshots.clone() {
            ui.push_id(&snapshot.path, |ui| {
                let title = directory_label(&snapshot);
                let time = chrono::DateTime::from_timestamp(snapshot.created, 0)
                    .map(|time| {
                        time.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default();
                history_row(ui, &title, &time, true, |ui| {
                    self.snapshot_actions(ui, id, &snapshot);
                });
            });
        }
    }

    fn paths(&mut self, ui: &mut egui::Ui, input: &GameInput) {
        ui.heading("追踪路径");
        if input.paths.is_empty() {
            ui.weak("尚未添加追踪路径。");
        }
        for path in &input.paths {
            ui.horizontal(|ui| {
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy();
                if path_row(ui, Some(&name), path, "打开位置").clicked() {
                    let result = if path.is_dir() {
                        platform::open_folder(path)
                    } else {
                        platform::select_file(path)
                    };
                    self.report(result, "已打开存档位置");
                }
            });
        }
        ui.separator();
        ui.heading("快照根目录");
        ui.horizontal(|ui| {
            if path_row(ui, None, &input.root, "打开目录").clicked() {
                self.report(platform::open_folder(&input.root), "已打开快照根目录");
            }
        });
    }

    fn snapshot_actions(&mut self, ui: &mut egui::Ui, id: Uuid, snapshot: &Snapshot) {
        if ui.button("打开目录").clicked() {
            self.report(platform::open_folder(&snapshot.path), "已打开快照目录");
        }
        ui.menu_button("更多", |ui| {
            ui.add_enabled_ui(self.recovery.is_none(), |ui| {
                if ui.button("恢复").clicked() {
                    ui.close();
                    match self.manager().restore_targets(id, snapshot) {
                        Ok(targets) => {
                            self.dialog = Some(Dialog::Restore(snapshot.clone(), targets))
                        }
                        Err(error) => self.report(Err(error), ""),
                    }
                }
                if ui.button("改名").clicked() {
                    ui.close();
                    self.dialog = Some(Dialog::Rename(
                        snapshot.clone(),
                        snapshot.name.clone().unwrap_or_default(),
                    ));
                }
                if ui
                    .button(RichText::new("删除").color(ui.visuals().error_fg_color))
                    .clicked()
                {
                    ui.close();
                    self.dialog = Some(Dialog::Delete(snapshot.clone()));
                }
            });
        });
    }

    fn edit(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.editor.take() else {
            return;
        };
        let mut save = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("game editor"))
            .frame(egui::Frame::popup(&ctx.style()).inner_margin(20))
            .show(ctx, |ui| {
                ui.set_min_width(525.);
                ui.heading(if editor.id.is_some() {
                    "编辑游戏配置"
                } else {
                    "添加游戏"
                });
                ui.label("游戏名称");
                ui.text_edit_singleline(&mut editor.name);
                ui.label("追踪路径");
                let mut remove = None;
                egui::ScrollArea::vertical()
                    .max_height(230.)
                    .show(ui, |ui| {
                        for (index, path) in editor.paths.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(path).desired_width(420.));
                                if ui.button("移除").clicked() {
                                    remove = Some(index);
                                }
                            });
                        }
                    });
                if let Some(index) = remove {
                    editor.paths.remove(index);
                }
                ui.horizontal(|ui| {
                    if ui.button("添加文件").clicked()
                        && let Some(paths) = rfd::FileDialog::new().pick_files()
                    {
                        editor
                            .paths
                            .extend(paths.iter().map(|path| path.display().to_string()));
                    }
                    if ui.button("添加目录").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        editor.paths.push(path.display().to_string());
                    }
                    if ui.button("输入路径").clicked() {
                        editor.paths.push(String::new());
                    }
                });
                ui.label("快照根目录");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut editor.root).desired_width(420.));
                    if ui.button("选择目录").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        editor.root = path.display().to_string();
                    }
                });
                if let Some((message, true)) = &self.status {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    save = theme::primary(ui, "保存", true).clicked();
                    close = ui.button("取消").clicked();
                });
            });
        close |= modal.should_close();
        if save {
            let input = GameInput {
                name: editor.name.clone(),
                root: PathBuf::from(editor.root.trim()),
                paths: editor
                    .paths
                    .iter()
                    .map(|path| PathBuf::from(path.trim()))
                    .collect(),
            };
            match self
                .manager
                .as_mut()
                .expect("idle")
                .save_game(editor.id, input)
            {
                Ok(id) => {
                    self.selected = Some(id);
                    self.report(Ok(()), "配置已保存");
                    self.refresh();
                    close = true;
                }
                Err(error) => self.report(Err(error), ""),
            }
        }
        if !close {
            self.editor = Some(editor);
        }
    }

    fn confirm(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let Some(id) = self.selected else { return };
        let mut confirmed = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("confirm operation"))
            .frame(egui::Frame::popup(&ctx.style()).inner_margin(20))
            .show(ctx, |ui| {
                ui.set_min_width(455.);
                let button = match &mut dialog {
                    Dialog::Rename(_, name) => {
                        ui.heading("修改快照名称");
                        ui.add(egui::TextEdit::singleline(name).desired_width(425.));
                        "保存"
                    }
                    Dialog::Delete(snapshot) => {
                        ui.heading("删除快照");
                        ui.label(format!(
                            "将快照 {} 移到 Windows 回收站？",
                            directory_label(snapshot)
                        ));
                        "移到回收站"
                    }
                    Dialog::Restore(snapshot, targets) => {
                        ui.heading(format!("恢复快照 {}", directory_label(snapshot)));
                        ui.label("恢复将完整替换这些存档。请先关闭游戏。");
                        show_targets(ui, targets);
                        "确认恢复"
                    }
                    Dialog::Remove => {
                        ui.heading("移除游戏配置");
                        ui.label("磁盘上的快照会保留。");
                        "移除"
                    }
                    Dialog::Recover(recovery) => {
                        if recovery.committed {
                            ui.heading("完成恢复清理");
                            ui.label("存档已恢复。确认后清理临时副本。");
                        } else {
                            ui.heading("恢复曾意外中断");
                            ui.label("确认后尝试撤回尚未完成的恢复。");
                        }
                        show_targets(ui, &recovery.targets);
                        if !recovery.backups.is_empty() {
                            ui.strong("原存档副本");
                            show_targets(ui, &recovery.backups);
                        }
                        "处理恢复记录"
                    }
                };
                if let Some((message, true)) = &self.status {
                    ui.colored_label(ui.visuals().error_fg_color, message);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    confirmed = if matches!(dialog, Dialog::Delete(_) | Dialog::Remove) {
                        ui.button(RichText::new(button).color(ui.visuals().error_fg_color))
                            .clicked()
                    } else {
                        theme::primary(ui, button, true).clicked()
                    };
                    close = ui.button("取消").clicked();
                });
            });
        close |= modal.should_close();
        if confirmed {
            match dialog {
                Dialog::Rename(ref snapshot, ref name) => {
                    let result = self.manager().rename_snapshot(id, snapshot, name);
                    close = result.is_ok();
                    self.report(result, "快照已改名");
                    if close {
                        self.refresh();
                    }
                }
                Dialog::Delete(snapshot) => {
                    self.start(ctx, Action::Delete, move |manager| {
                        manager.delete_snapshot(id, &snapshot)
                    });
                    return;
                }
                Dialog::Restore(snapshot, targets) => {
                    self.start(ctx, Action::Restore, move |manager| {
                        manager.restore_snapshot(id, &snapshot, &targets)
                    });
                    return;
                }
                Dialog::Remove => {
                    let result = self.manager.as_mut().expect("idle").remove_game(id);
                    close = result.is_ok();
                    self.report(result, "游戏配置已移除");
                    if close {
                        self.selected = self.manager().games().first().map(|game| game.id);
                        self.name.clear();
                        self.refresh();
                    }
                }
                Dialog::Recover(_) => {
                    self.start(ctx, Action::Recover, move |manager| {
                        manager.recover_restore(id)
                    });
                    return;
                }
            }
        }
        if !close {
            self.dialog = Some(dialog);
        }
    }
}

fn path_row(ui: &mut egui::Ui, name: Option<&str>, path: &Path, button: &str) -> egui::Response {
    let path = path.display().to_string();
    let width = (ui.available_width() - 100.).max(45.);
    ui.allocate_ui_with_layout(Vec2::new(width, 0.), Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 4.;
        if let Some(name) = name {
            ui.add(egui::Label::new(RichText::new(name).size(16.).strong()).truncate())
                .on_hover_text(name);
        }
        let path_text = RichText::new(&path).size(14.);
        let path_text = if name.is_some() {
            path_text.weak()
        } else {
            path_text
        };
        ui.add(egui::Label::new(path_text).truncate())
            .on_hover_text(&path);
    });
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.button(button))
        .inner
}

fn history_row(
    ui: &mut egui::Ui,
    name: &str,
    time: &str,
    record: bool,
    actions: impl FnOnce(&mut egui::Ui),
) {
    let width = ui.available_width();
    let control_height = ui.spacing().interact_size.y;
    let height = if record {
        control_height + 12.
    } else {
        control_height
    };
    let (row, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    if record {
        ui.painter()
            .rect_filled(row, 9, ui.visuals().faint_bg_color);
    }
    let row = row.shrink2(egui::vec2(10., 0.));
    let gap = ui.spacing().item_spacing.x;
    let actions_left = row.right() - 142.;
    let time_left = actions_left - gap - 160.;
    let name_right = (time_left - gap).max(row.left());

    let cell = |id, left, right| {
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(left, row.top()),
                egui::pos2(right, row.bottom()),
            ))
            .layout(Layout::left_to_right(Align::Center))
    };
    {
        let mut ui = ui.new_child(cell("name", row.left(), name_right));
        let label = if record {
            RichText::new(name).strong()
        } else {
            RichText::new(name).weak()
        };
        ui.add(egui::Label::new(label).truncate())
            .on_hover_text(name);
    }
    ui.new_child(cell("time", time_left, actions_left - gap))
        .weak(time);
    actions(&mut ui.new_child(cell("actions", actions_left, row.right())));
}

fn directory_label(snapshot: &Snapshot) -> String {
    snapshot
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn show_targets(ui: &mut egui::Ui, paths: &[PathBuf]) {
    egui::ScrollArea::vertical()
        .id_salt(paths)
        .max_height(150.)
        .show(ui, |ui| {
            for path in paths {
                ui.label(Path::display(path).to_string());
            }
        });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some((kind, receiver)) = &self.job {
            match receiver.try_recv() {
                Ok((manager, result)) => {
                    let kind = *kind;
                    self.manager = Some(manager);
                    self.job = None;
                    if result.is_ok() && matches!(kind, Action::Create) {
                        self.name.clear();
                    }
                    self.report(result, kind.success());
                    self.refresh();
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.job = None;
                    self.failure = Some("操作意外中断，请重启应用。".into());
                }
            }
        }
        if self.job.is_some() && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if self.manager.is_none() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| match &self.failure {
                    Some(error) => {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    None => {
                        ui.label("正在处理，请稍候…");
                    }
                });
            });
            return;
        }
        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("存档管理器");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(appearance) = theme::toggle(ui) {
                            if let Err(error) = self.manager().save_appearance(appearance) {
                                self.report(Err(error), "");
                            } else {
                                ctx.set_theme(theme::preference(Some(appearance)));
                                ctx.request_repaint();
                            }
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if let Some((message, error)) = &self.status {
                let text = RichText::new(message);
                ui.label(if *error {
                    text.color(ui.visuals().error_fg_color)
                } else {
                    text
                });
            }
        });
        self.sidebar(ctx);
        self.workspace(ctx);
        if self.job.is_none() {
            self.edit(ctx);
            self.confirm(ctx);
        }
    }
}
