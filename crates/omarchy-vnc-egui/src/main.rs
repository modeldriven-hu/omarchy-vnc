use eframe::egui::{self, Color32, Key, TextureHandle, TextureOptions};
use omarchy_vnc_core::{Input, Model, Palette, Session, Status, Viewport, keysym};
use std::{collections::BTreeMap, sync::Arc};
use zeroize::Zeroize;

fn main() {
    if let Err(error) = run() {
        eprintln!("Omarchy VNC: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let options = omarchy_vnc_core::startup_options()?;
    let mut model = Model::load_launch(&options).map_err(|e| e.to_string())?;
    if options.gui {
        return run_window(model, None, false)
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    if !omarchy_vnc_tui::ensure_terminal().map_err(|e| e.to_string())? {
        return Ok(());
    }
    loop {
        let Some(session) =
            omarchy_vnc_tui::connect(&mut model, "egui / OpenGL").map_err(|e| e.to_string())?
        else {
            return Ok(());
        };
        match run_window(model.clone(), Some(session), true) {
            Ok(Some(error)) => model.error = error,
            Ok(None) => {}
            Err(error) => model.error = format!("Cannot open the graphical viewer: {error}"),
        }
    }
}
fn run_window(
    model: Model,
    mut initial: Option<Session>,
    viewer_only: bool,
) -> eframe::Result<Option<String>> {
    let exit_notice = std::rc::Rc::new(std::cell::RefCell::new(None));
    let notice = exit_notice.clone();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        run_and_return: true,
        viewport: egui::ViewportBuilder::default()
            .with_title("Omarchy VNC · egui")
            .with_app_id("local.omarchy-vnc.egui")
            .with_inner_size([1100., 740.]),
        ..Default::default()
    };
    eframe::run_native(
        "Omarchy VNC · egui",
        options,
        Box::new(move |cc| {
            let p = Palette::load();
            let c = |v: [u8; 3]| Color32::from_rgb(v[0], v[1], v[2]);
            let mut v = if p.background.iter().map(|v| *v as u32).sum::<u32>() > 420 {
                egui::Visuals::light()
            } else {
                egui::Visuals::dark()
            };
            v.panel_fill = c(p.background);
            v.window_fill = c(p.background);
            v.override_text_color = Some(c(p.foreground));
            v.selection.bg_fill = c(p.accent);
            cc.egui_ctx.set_visuals(v);
            if let Some(session) = &mut initial {
                let ctx = cc.egui_ctx.clone();
                session.set_waker(Arc::new(move || ctx.request_repaint()));
            }
            let fullscreen = initial.is_some() && model.draft.fullscreen;
            if fullscreen {
                cc.egui_ctx
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            }
            Ok(Box::new(App {
                viewer_only,
                exit_notice: notice,
                model,
                password: String::new(),
                password_visible: false,
                session: initial,
                texture: None,
                cursor_texture: None,
                cursor_generation: None,
                generation: None,
                remote_rect: None,
                pointer: (0, 0),
                fit: true,
                fullscreen,
                captured: false,
                keys: BTreeMap::new(),
                modifiers: egui::Modifiers::NONE,
                delete_confirm: false,
                clipboard: arboard::Clipboard::new().ok(),
                last_clipboard: String::new(),
            }))
        }),
    )?;
    let message = exit_notice.borrow_mut().take();
    Ok(message)
}
struct App {
    viewer_only: bool,
    exit_notice: std::rc::Rc<std::cell::RefCell<Option<String>>>,
    model: Model,
    password: String,
    password_visible: bool,
    session: Option<Session>,
    texture: Option<TextureHandle>,
    cursor_texture: Option<TextureHandle>,
    cursor_generation: Option<u64>,
    generation: Option<u64>,
    remote_rect: Option<egui::Rect>,
    pointer: (u16, u16),
    fit: bool,
    fullscreen: bool,
    captured: bool,
    keys: BTreeMap<Key, u32>,
    modifiers: egui::Modifiers,
    delete_confirm: bool,
    clipboard: Option<arboard::Clipboard>,
    last_clipboard: String,
}
impl Drop for App {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}
impl App {
    fn release(&mut self) {
        if let Some(s) = &mut self.session {
            s.send(Input::Release);
        }
        self.keys.clear();
        self.modifiers = egui::Modifiers::NONE;
        self.captured = false;
    }
    fn outcome(&mut self, r: Result<(), impl std::fmt::Display>) {
        self.model.error = r.err().map(|e| e.to_string()).unwrap_or_default();
    }
    fn connect(&mut self, ctx: &egui::Context) {
        self.release();
        self.session = None;
        self.texture = None;
        self.cursor_texture = None;
        self.cursor_generation = None;
        self.generation = None;
        let context = ctx.clone();
        self.password_visible = false;
        let password = std::mem::take(&mut self.password);
        match Session::connect(
            self.model.draft.clone(),
            password,
            Arc::new(move || context.request_repaint()),
        ) {
            Ok(s) => {
                self.session = Some(s);
                self.model.error.clear();
                self.fullscreen = self.model.draft.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Err(e) => self.model.error = e.to_string(),
        }
    }
    fn remote_input(&mut self, ctx: &egui::Context, origin: egui::Pos2, viewport: Viewport) {
        if !self.captured {
            return;
        }
        let (events, mods, focused) = ctx.input(|i| (i.events.clone(), i.modifiers, i.focused));
        if !focused
            || ctx.input(|i| i.key_pressed(Key::Escape) && i.modifiers.ctrl && i.modifiers.alt)
        {
            self.release();
            self.fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            return;
        }
        let Some(session) = &mut self.session else {
            return;
        };
        for (before, now, k) in [
            (
                self.modifiers.ctrl,
                mods.ctrl,
                if self.model.draft.map_ctrl_to_command {
                    0xffeb
                } else {
                    0xffe3
                },
            ),
            (self.modifiers.shift, mods.shift, 0xffe1),
            (self.modifiers.alt, mods.alt, 0xffe9),
            (self.modifiers.mac_cmd, mods.mac_cmd, 0xffeb),
        ] {
            if before != now {
                session.send(Input::Key(k, now));
            }
        }
        self.modifiers = mods;
        for event in events {
            match event {
                egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    ..
                } => {
                    if pressed {
                        if (!(key.name().len() == 1 || key == Key::Space)
                            || mods.ctrl
                            || mods.alt
                            || mods.mac_cmd)
                            && let Some(k) = keycode(key, mods.shift)
                        {
                            self.keys.insert(key, k);
                            session.send(Input::Key(k, true));
                        }
                    } else if let Some(k) = self.keys.remove(&key) {
                        session.send(Input::Key(k, false));
                    }
                    if repeat {
                        ctx.request_repaint();
                    }
                }
                egui::Event::Text(t) => {
                    for ch in t.chars().filter(|c| !c.is_control()) {
                        session.send(Input::Key(keysym(ch), true));
                        session.send(Input::Key(keysym(ch), false));
                    }
                }
                egui::Event::Copy => {
                    session.send(Input::Key(99, true));
                    session.send(Input::Key(99, false));
                }
                egui::Event::Cut => {
                    session.send(Input::Key(120, true));
                    session.send(Input::Key(120, false));
                }
                egui::Event::Paste(_) => {
                    session.send(Input::Key(118, true));
                    session.send(Input::Key(118, false));
                }
                egui::Event::PointerMoved(pos) => {
                    if let Some((x, y)) = viewport.point(pos.x - origin.x, pos.y - origin.y) {
                        self.pointer = (x, y);
                        let buttons = ctx.input(|i| {
                            (u8::from(i.pointer.primary_down()))
                                | (u8::from(i.pointer.middle_down()) << 1)
                                | (u8::from(i.pointer.secondary_down()) << 2)
                        });
                        session.send(Input::Pointer(x, y, buttons));
                    }
                }
                egui::Event::PointerButton { pos, .. } => {
                    let (x, y) = viewport
                        .point(pos.x - origin.x, pos.y - origin.y)
                        .unwrap_or(self.pointer);
                    self.pointer = (x, y);
                    let buttons = ctx.input(|i| {
                        u8::from(i.pointer.primary_down())
                            | (u8::from(i.pointer.middle_down()) << 1)
                            | (u8::from(i.pointer.secondary_down()) << 2)
                    });
                    session.send(Input::Pointer(x, y, buttons));
                }
                egui::Event::MouseWheel { delta, .. } => {
                    if let Some(pos) = ctx.input(|i| i.pointer.hover_pos())
                        && let Some((x, y)) = viewport.point(pos.x - origin.x, pos.y - origin.y)
                    {
                        let b = if delta.y > 0. { 8 } else { 16 };
                        session.send(Input::Pointer(x, y, b));
                        session.send(Input::Pointer(x, y, 0));
                    }
                }
                _ => {}
            }
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.captured
            && ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| self.remote_rect.is_none_or(|r| !r.contains(p)))
            })
        {
            self.release();
        }
        let snapshot = self.session.as_ref().map(|s| s.snapshot());
        if self.viewer_only
            && !snapshot
                .as_ref()
                .is_some_and(|s| s.status == Status::Connected)
        {
            if let Some(s) = &snapshot
                && let Status::Failed(error) = &s.status
            {
                *self.exit_notice.borrow_mut() = Some(error.clone());
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let connected = snapshot
            .as_ref()
            .is_some_and(|s| s.status == Status::Connected);
        if !connected
            && !snapshot
                .as_ref()
                .is_some_and(|s| s.status == Status::Connecting)
            && ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::Enter))
        {
            self.connect(ctx);
        }
        if !connected && self.captured {
            self.release();
        }
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Omarchy VNC");
                ui.separator();
                if let Some(s) = &snapshot {
                    ui.label(match &s.status {
                        Status::Connecting => "Connecting…".into(),
                        Status::Connected => "Connected · unencrypted VNC".into(),
                        Status::Disconnected => "Disconnected".into(),
                        Status::Failed(e) => format!("Connection failed: {e}"),
                    });
                }
                if self.session.is_some() && ui.button("Disconnect / Cancel").clicked() {
                    self.release();
                    self.session = None;
                    self.texture = None;
                    self.generation = None;
                    self.fullscreen = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                    if self.viewer_only {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                if connected {
                    ui.checkbox(&mut self.fit, "Fit to window");
                    if ui.checkbox(&mut self.fullscreen, "Fullscreen").changed() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                    }
                    if ui.button("Release input").clicked() {
                        self.release();
                    }
                    ui.label("Ctrl+Alt+Esc releases input");
                }
            });
            if !self.model.error.is_empty() {
                ui.colored_label(Color32::from_rgb(240, 110, 110), &self.model.error);
            }
        });
        if !connected {
            egui::SidePanel::left("profiles")
                .min_width(210.)
                .show(ctx, |ui| {
                    ui.heading("Connections");
                    for i in 0..self.model.profiles.profiles.len() {
                        let p = &self.model.profiles.profiles[i];
                        let title = format!(
                            "{}{}",
                            if self.model.profiles.default.as_ref() == Some(&p.name) {
                                "★ "
                            } else {
                                ""
                            },
                            p.name
                        );
                        if ui
                            .selectable_label(self.model.selected == Some(i), title)
                            .clicked()
                        {
                            self.model.select(i);
                            self.password.zeroize();
                        }
                    }
                    ui.separator();
                    if ui.button("New connection").clicked() {
                        self.model.new_profile();
                        self.password.zeroize();
                    }
                    if ui.button("Reload saved profiles").clicked() {
                        let r = self.model.reload();
                        self.outcome(r);
                    }
                });
        }
        egui::CentralPanel::default().show(ctx,|ui| {
            if connected {
                if let Some(frame)=snapshot.as_ref().and_then(|s|s.frame.as_ref()) {
                    if self.generation!=Some(frame.generation) {let image=egui::ColorImage::from_rgba_unmultiplied([frame.width,frame.height],&frame.rgba);if let Some(t)=&mut self.texture {t.set(image,TextureOptions::LINEAR);}else{self.texture=Some(ctx.load_texture("remote",image,TextureOptions::LINEAR));}self.generation=Some(frame.generation);}
                    egui::ScrollArea::both().show(ui,|ui| {
                        let size=ui.available_size();let vp=Viewport::fit([size.x,size.y],[frame.width,frame.height],self.fit);
                        let origin=ui.cursor().min;
                        let rect=egui::Rect::from_min_size(origin+egui::vec2(vp.x,vp.y),egui::vec2(vp.width,vp.height));
                        ui.allocate_space(egui::vec2(size.x.max(vp.width),size.y.max(vp.height)));
                        if let Some(t)=&self.texture {ui.painter().image(t.id(),rect,egui::Rect::from_min_max(egui::pos2(0.,0.),egui::pos2(1.,1.)),Color32::WHITE);}
                        let response=ui.interact(rect,ui.id().with("remote"),egui::Sense::click_and_drag());
                        self.remote_rect=Some(rect);
                        if response.hovered() {
                            if let Some(cursor)=snapshot.as_ref().and_then(|s|s.cursor.as_ref()) {
                                if self.cursor_generation!=Some(cursor.generation) {
                                    let image=egui::ColorImage::from_rgba_unmultiplied([cursor.width,cursor.height],&cursor.rgba);
                                    if let Some(texture)=&mut self.cursor_texture {texture.set(image,TextureOptions::NEAREST);} else {self.cursor_texture=Some(ctx.load_texture("remote-cursor",image,TextureOptions::NEAREST));}
                                    self.cursor_generation=Some(cursor.generation);
                                }
                                if let (Some(position),Some(texture))=(ctx.input(|i|i.pointer.hover_pos()),self.cursor_texture.as_ref()) {
                                    let scale=vp.width/frame.width as f32;
                                    let min=position-egui::vec2(cursor.hotspot[0] as f32*scale,cursor.hotspot[1] as f32*scale);
                                    let cursor_rect=egui::Rect::from_min_size(min,egui::vec2(cursor.width as f32*scale,cursor.height as f32*scale));
                                    ui.painter().with_clip_rect(rect).image(texture.id(),cursor_rect,egui::Rect::from_min_max(egui::pos2(0.,0.),egui::pos2(1.,1.)),Color32::WHITE);
                                    ctx.set_cursor_icon(egui::CursorIcon::None);
                                }
                            } else {ctx.set_cursor_icon(egui::CursorIcon::Default);}
                        }
                        if response.is_pointer_button_down_on() || response.clicked() || response.drag_started() {self.captured=true;response.request_focus();}
                        self.remote_input(ctx,origin,vp);
                    });
                    if !self.captured {ui.label("Click the remote desktop to send keyboard and pointer input.");}
                } else {ui.spinner();ui.label("Waiting for the first frame…");}
                if self.model.draft.clipboard&& let Some(text)=self.session.as_ref().and_then(|s|s.take_clipboard())&& self.last_clipboard!=text {if let Some(c)=&mut self.clipboard {let _=c.set_text(text.clone());}self.last_clipboard=text;}
            } else {
                ui.heading(if self.model.selected.is_some(){"Edit connection"}else{"New connection"});ui.add_space(12.);
                egui::Grid::new("form").num_columns(2).min_col_width(150.).spacing([18.,16.]).show(ui,|ui| {
                    ui.label("Name");ui.add(egui::TextEdit::singleline(&mut self.model.draft.name).desired_width(380.).min_size(egui::vec2(380.,32.)));ui.end_row();
                    ui.label("Host");ui.add(egui::TextEdit::singleline(&mut self.model.draft.host).hint_text("macmini.local").desired_width(380.).min_size(egui::vec2(380.,32.)));ui.end_row();
                    ui.label("Port");ui.add_sized([120.,32.],egui::DragValue::new(&mut self.model.draft.port).range(1..=65535));ui.end_row();
                    ui.label("Mac username");
                    ui.add(egui::TextEdit::singleline(&mut self.model.draft.username).desired_width(380.).min_size(egui::vec2(380.,32.)));ui.end_row();
                    ui.label("Password");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.password).password(!self.password_visible).desired_width(380.).min_size(egui::vec2(380.,32.)));
                        if ui.button(if self.password_visible {"Hide"} else {"Show"}).clicked() { self.password_visible = !self.password_visible; }
                    });ui.end_row();
                });
                ui.add_space(12.);ui.checkbox(&mut self.model.draft.fullscreen,"Start fullscreen");
                ui.checkbox(&mut self.model.draft.map_ctrl_to_command,"Map Ctrl to Mac Command (Super stays local)");
                ui.checkbox(&mut self.model.draft.clipboard,"Allow text clipboard (Latin-1, explicit send; receive automatically)");
                ui.label("VNC traffic is unencrypted. Use a trusted LAN or a separately configured secure tunnel.");
                ui.label("For Apple login, use the Mac account’s short username and password. Leave username blank for standard VNC authentication. Passwords are never saved.");
                ui.add_space(16.);ui.horizontal(|ui| {
                    if ui.button("Save profile").clicked(){let r=self.model.save();self.outcome(r);}
                    if ui.add_enabled(self.model.selected.is_some(),egui::Button::new("Make default")).clicked(){let r=self.model.make_default();self.outcome(r);}
                    if ui.add_enabled(self.model.selected.is_some(),egui::Button::new("Delete…")).clicked(){self.delete_confirm=true;}
                    if ui.add_enabled(!snapshot.as_ref().is_some_and(|s|s.status==Status::Connecting),egui::Button::new("Connect")).clicked(){self.connect(ctx);}
                });
            }
        });
        if connected && self.model.draft.clipboard {
            egui::TopBottomPanel::bottom("clipboard").show(ctx, |ui| {
                if ui.button("Send local clipboard to Mac").clicked()
                    && let Some(c) = &mut self.clipboard
                {
                    match c.get_text() {
                        Ok(t) => {
                            if let Some(s) = &mut self.session {
                                s.send(Input::Clipboard(t));
                            }
                        }
                        Err(e) => self.model.error = e.to_string(),
                    }
                }
            });
        }
        if self.delete_confirm {
            egui::Window::new("Delete saved connection?")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("This removes the saved profile from both frontends.");
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            let r = self.model.delete();
                            self.outcome(r);
                            self.delete_confirm = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.delete_confirm = false;
                        }
                    });
                });
        }
    }
}
fn keycode(k: Key, shift: bool) -> Option<u32> {
    Some(match k {
        Key::ArrowLeft => 0xff51,
        Key::ArrowUp => 0xff52,
        Key::ArrowRight => 0xff53,
        Key::ArrowDown => 0xff54,
        Key::Escape => 0xff1b,
        Key::Tab => 0xff09,
        Key::Backspace => 0xff08,
        Key::Enter => 0xff0d,
        Key::Space => 32,
        Key::Insert => 0xff63,
        Key::Delete => 0xffff,
        Key::Home => 0xff50,
        Key::End => 0xff57,
        Key::PageUp => 0xff55,
        Key::PageDown => 0xff56,
        Key::F1 => 0xffbe,
        Key::F2 => 0xffbf,
        Key::F3 => 0xffc0,
        Key::F4 => 0xffc1,
        Key::F5 => 0xffc2,
        Key::F6 => 0xffc3,
        Key::F7 => 0xffc4,
        Key::F8 => 0xffc5,
        Key::F9 => 0xffc6,
        Key::F10 => 0xffc7,
        Key::F11 => 0xffc8,
        Key::F12 => 0xffc9,
        _ => {
            let name = k.name();
            if name.len() != 1 {
                return None;
            }
            let c = name.chars().next()?;
            let c = if c.is_ascii_alphabetic() && !shift {
                c.to_ascii_lowercase()
            } else {
                c
            };
            c as u32
        }
    })
}
