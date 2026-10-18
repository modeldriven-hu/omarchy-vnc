use gtk::{gdk, glib, prelude::*};
use omarchy_vnc_core::{Input, Model, Palette, Session, Status, Viewport};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{Read, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    rc::Rc,
    sync::{Arc, Mutex},
};
use zeroize::Zeroize;

struct State {
    model: Model,
    session: Option<Session>,
    generation: Option<u64>,
    cursor_generation: Option<u64>,
    captured: bool,
    buttons: u8,
    point: (u16, u16),
    keys: BTreeMap<u32, u32>,
    dimensions: [usize; 2],
}
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
        return run_window(model, None, false).map(|_| ());
    }
    if !omarchy_vnc_tui::ensure_terminal().map_err(|e| e.to_string())? {
        return Ok(());
    }
    loop {
        let Some(session) =
            omarchy_vnc_tui::connect(&mut model, "GTK4").map_err(|e| e.to_string())?
        else {
            return Ok(());
        };
        match run_window(model.clone(), Some(session), true) {
            Ok(Some(error)) => model.error = error,
            Ok(None) => {}
            Err(error) => model.error = error,
        }
    }
}
fn run_window(
    model: Model,
    initial: Option<Session>,
    viewer_only: bool,
) -> Result<Option<String>, String> {
    gtk::init().map_err(|e| format!("Cannot initialize GTK: {e}"))?;
    let notice = Rc::new(RefCell::new(None));
    let result = notice.clone();
    let payload = RefCell::new(Some((model, initial)));
    let app = gtk::Application::builder()
        .application_id("local.omarchy_vnc.gtk")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        if let Some((model, session)) = payload.borrow_mut().take() {
            build(app, model, session, viewer_only, notice.clone());
        } else if let Some(window) = app.active_window() {
            window.present();
        }
    });
    app.run_with_args(&["omarchy-vnc-gtk"]);
    let message = result.borrow_mut().take();
    Ok(message)
}

fn release(state: &Rc<RefCell<State>>) {
    let mut s = state.borrow_mut();
    if let Some(session) = &mut s.session {
        session.send(Input::Release);
    }
    s.keys.clear();
    s.buttons = 0;
    s.captured = false;
}
fn build(
    app: &gtk::Application,
    model: Model,
    initial: Option<Session>,
    viewer_only: bool,
    exit_notice: Rc<RefCell<Option<String>>>,
) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Omarchy VNC · GTK")
        .default_width(1100)
        .default_height(740)
        .build();
    let p = Palette::load();
    let rgb = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let css = gtk::CssProvider::new();
    css.load_from_data(&format!("window {{ background: {}; color: {}; }} button:focus, entry:focus {{ outline: 2px solid {}; }} .toolbar {{ padding: 8px; }}",rgb(p.background),rgb(p.foreground),rgb(p.accent)));
    let display = gdk::Display::default().expect("GTK display");
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    app.connect_shutdown(move |_| gtk::style_context_remove_provider_for_display(&display, &css));
    let state = Rc::new(RefCell::new(State {
        model,
        session: initial,
        generation: None,
        cursor_generation: None,
        captured: false,
        buttons: 0,
        point: (0, 0),
        keys: BTreeMap::new(),
        dimensions: [1, 1],
    }));
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    window.set_child(Some(&root));
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.add_css_class("toolbar");
    root.append(&toolbar);
    let title = gtk::Label::new(Some("Omarchy VNC"));
    title.add_css_class("title-2");
    toolbar.append(&title);
    let status = gtk::Label::new(Some("Choose or add a connection"));
    status.set_hexpand(true);
    status.set_xalign(0.);
    toolbar.append(&status);
    let disconnect = gtk::Button::with_label("Disconnect / Cancel");
    toolbar.append(&disconnect);
    let fit = gtk::CheckButton::with_label("Fit");
    fit.set_active(true);
    toolbar.append(&fit);
    let fullscreen = gtk::ToggleButton::with_label("Fullscreen");
    toolbar.append(&fullscreen);
    let release_button = gtk::Button::with_label("Release input");
    toolbar.append(&release_button);
    let error = gtk::Label::new(None);
    error.set_wrap(true);
    error.set_xalign(0.);
    error.add_css_class("error");
    root.append(&error);
    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    root.append(&stack);
    let form = gtk::Box::new(gtk::Orientation::Vertical, 12);
    form.set_margin_top(16);
    form.set_margin_bottom(16);
    form.set_margin_start(16);
    form.set_margin_end(16);
    stack.add_named(&form, Some("profiles"));
    let selector = gtk::ComboBoxText::new();
    form.append(&selector);
    let name = gtk::Entry::builder()
        .placeholder_text("Connection name")
        .build();
    let host = gtk::Entry::builder()
        .placeholder_text("Hostname or IP (for example macmini.local)")
        .build();
    let port = gtk::SpinButton::with_range(1., 65535., 1.);
    let username = gtk::Entry::builder()
        .placeholder_text("Mac account short username (blank for standard VNC)")
        .build();
    let password = gtk::Entry::builder()
        .visibility(false)
        .input_purpose(gtk::InputPurpose::Password)
        .placeholder_text("Password (never saved)")
        .build();
    let password_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    password.set_hexpand(true);
    let show_password = gtk::ToggleButton::with_label("Show");
    show_password.connect_toggled({
        let password = password.clone();
        move |button| {
            password.set_visibility(button.is_active());
            button.set_label(if button.is_active() { "Hide" } else { "Show" });
        }
    });
    password_row.append(&password);
    password_row.append(&show_password);
    for entry in [&name, &host, &username, &password] {
        entry.set_height_request(36);
        entry.set_width_chars(30);
    }
    for (label, widget) in [
        ("Name", name.upcast_ref::<gtk::Widget>()),
        ("Host", host.upcast_ref()),
        ("Port", port.upcast_ref()),
        ("Mac username", username.upcast_ref()),
        ("Password", password_row.upcast_ref()),
    ] {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let l = gtk::Label::new(Some(label));
        l.set_width_chars(14);
        l.set_xalign(0.);
        row.append(&l);
        widget.set_hexpand(true);
        row.append(widget);
        form.append(&row);
    }
    let start_fullscreen = gtk::CheckButton::with_label("Start fullscreen");
    form.append(&start_fullscreen);
    let command_mapping =
        gtk::CheckButton::with_label("Map Ctrl to Mac Command (Super stays local)");
    form.append(&command_mapping);
    let clipboard = gtk::CheckButton::with_label(
        "Allow Latin-1 text clipboard (explicit send; automatic receive)",
    );
    form.append(&clipboard);
    let note = gtk::Label::new(Some(
        "Unencrypted VNC: use a trusted LAN or a separately configured secure tunnel.\nFor Apple login use the Mac account’s short username and password. Leave username blank for standard VNC.",
    ));
    note.set_wrap(true);
    note.set_xalign(0.);
    form.append(&note);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    form.append(&actions);
    let new = gtk::Button::with_label("New");
    let save = gtk::Button::with_label("Save");
    let default = gtk::Button::with_label("Make default");
    let delete = gtk::Button::with_label("Delete…");
    let reload = gtk::Button::with_label("Reload");
    let connect = gtk::Button::with_label("Connect");
    for button in [&new, &save, &default, &delete, &reload, &connect] {
        actions.append(button);
    }
    let remote_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    stack.add_named(&remote_box, Some("remote"));
    let picture = gtk::Picture::new();
    picture.set_can_shrink(true);
    picture.set_keep_aspect_ratio(true);
    picture.set_focusable(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    let scroll = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&picture)
        .build();
    remote_box.append(&scroll);
    let hint = gtk::Label::new(Some(
        "Click the remote desktop to capture input · Ctrl+Alt+Escape releases input and leaves fullscreen",
    ));
    remote_box.append(&hint);
    let send_clipboard = gtk::Button::with_label("Send local clipboard to Mac");
    remote_box.append(&send_clipboard);
    let fill: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let name = name.clone();
        let username = username.clone();
        let host = host.clone();
        let port = port.clone();
        let full = start_fullscreen.clone();
        let clipboard = clipboard.clone();
        let command_mapping = command_mapping.clone();
        move || {
            let s = state.borrow();
            let p = &s.model.draft;
            name.set_text(&p.name);
            username.set_text(&p.username);
            host.set_text(&p.host);
            port.set_value(p.port.into());
            full.set_active(p.fullscreen);
            clipboard.set_active(p.clipboard);
            command_mapping.set_active(p.map_ctrl_to_command);
        }
    });
    let refresh: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        let selector = selector.clone();
        move || {
            let (profiles, default, selected) = {
                let s = state.borrow();
                (
                    s.model.profiles.profiles.clone(),
                    s.model.profiles.default.clone(),
                    s.model
                        .selected
                        .map(|i| s.model.profiles.profiles[i].name.clone()),
                )
            };
            selector.remove_all();
            for p in &profiles {
                selector.append(
                    Some(&p.name),
                    &format!(
                        "{}{}",
                        if default.as_ref() == Some(&p.name) {
                            "★ "
                        } else {
                            ""
                        },
                        p.name
                    ),
                );
            }
            if let Some(selected) = selected {
                selector.set_active_id(Some(&selected));
            }
        }
    });
    refresh();
    fill();
    error.set_text(&state.borrow().model.error);
    selector.connect_changed({
        let state = state.clone();
        let fill = fill.clone();
        let password = password.clone();
        move |c| {
            if let Some(id) = c.active_id() {
                let index = state
                    .borrow()
                    .model
                    .profiles
                    .profiles
                    .iter()
                    .position(|p| p.name == id);
                if let Some(i) = index {
                    state.borrow_mut().model.select(i);
                    password.set_text("");
                    fill();
                }
            }
        }
    });
    new.connect_clicked({
        let state = state.clone();
        let fill = fill.clone();
        let password = password.clone();
        move |_| {
            state.borrow_mut().model.new_profile();
            password.set_text("");
            fill();
        }
    });
    let read_form: Rc<dyn Fn()> = Rc::new({
        let state = state.clone();
        move || {
            let mut s = state.borrow_mut();
            s.model.draft.name = name.text().to_string();
            s.model.draft.username = username.text().to_string();
            s.model.draft.host = host.text().to_string();
            s.model.draft.port = port.value_as_int() as u16;
            s.model.draft.fullscreen = start_fullscreen.is_active();
            s.model.draft.clipboard = clipboard.is_active();
            s.model.draft.map_ctrl_to_command = command_mapping.is_active();
        }
    });
    save.connect_clicked({
        let state = state.clone();
        let read = read_form.clone();
        let refresh = refresh.clone();
        let error = error.clone();
        move |_| {
            read();
            let result = state.borrow_mut().model.save();
            error.set_text(&result.err().map(|e| e.to_string()).unwrap_or_default());
            refresh();
        }
    });
    default.connect_clicked({
        let state = state.clone();
        let refresh = refresh.clone();
        let error = error.clone();
        move |_| {
            let result = state.borrow_mut().model.make_default();
            error.set_text(&result.err().map(|e| e.to_string()).unwrap_or_default());
            refresh();
        }
    });
    reload.connect_clicked({
        let state = state.clone();
        let refresh = refresh.clone();
        let fill = fill.clone();
        let error = error.clone();
        move |_| {
            let result = state.borrow_mut().model.reload();
            error.set_text(&result.err().map(|e| e.to_string()).unwrap_or_default());
            refresh();
            fill();
        }
    });
    delete.connect_clicked({
        let state = state.clone();
        let refresh = refresh.clone();
        let fill = fill.clone();
        let error = error.clone();
        let window = window.clone();
        move |_| {
            if state.borrow().model.selected.is_none() {
                return;
            }
            let dialog = gtk::MessageDialog::builder()
                .transient_for(&window)
                .modal(true)
                .message_type(gtk::MessageType::Question)
                .buttons(gtk::ButtonsType::OkCancel)
                .text("Delete this saved connection from both frontends?")
                .build();
            dialog.connect_response({
                let state = state.clone();
                let refresh = refresh.clone();
                let fill = fill.clone();
                let error = error.clone();
                move |d, response| {
                    if response == gtk::ResponseType::Ok {
                        let result = state.borrow_mut().model.delete();
                        error.set_text(&result.err().map(|e| e.to_string()).unwrap_or_default());
                        refresh();
                        fill();
                    }
                    d.close();
                }
            });
            dialog.present();
        }
    });
    let (mut reader, writer) = UnixStream::pair().expect("UI notification pipe");
    reader.set_nonblocking(true).unwrap();
    writer.set_nonblocking(true).unwrap();
    let writer = Arc::new(Mutex::new(writer));
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = writer.lock().unwrap().write(&[1]);
    });
    if let Some(session) = &mut state.borrow_mut().session {
        session.set_waker(wake.clone());
    }
    let generation = state.clone();
    let pic = picture.clone();
    let stack_update = stack.clone();
    let status_update = status.clone();
    let form_update = form.clone();
    let connect_update = connect.clone();
    let fit_update = fit.clone();
    let send_update = send_clipboard.clone();
    let window_weak = window.downgrade();
    let source =
        glib::source::unix_fd_add_local(reader.as_raw_fd(), glib::IOCondition::IN, move |_, _| {
            let mut bytes = [0; 256];
            while reader.read(&mut bytes).is_ok_and(|n| n > 0) {}
            if window_weak.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            let snapshot = generation.borrow().session.as_ref().map(|s| s.snapshot());
            if let Some(snapshot) = snapshot {
                if viewer_only && snapshot.status != Status::Connected {
                    if let Status::Failed(error) = snapshot.status {
                        *exit_notice.borrow_mut() = Some(error);
                    }
                    if let Some(window) = window_weak.upgrade() {
                        window.close();
                    }
                    return glib::ControlFlow::Continue;
                }
                status_update.set_text(&match &snapshot.status {
                    Status::Connecting => "Connecting…".into(),
                    Status::Connected => "Connected · unencrypted VNC".into(),
                    Status::Disconnected => "Disconnected".into(),
                    Status::Failed(e) => format!("Connection failed: {e}"),
                });
                form_update.set_sensitive(snapshot.status != Status::Connecting);
                connect_update.set_sensitive(snapshot.status != Status::Connecting);
                if snapshot.status == Status::Connected {
                    stack_update.set_visible_child_name("remote");
                } else {
                    stack_update.set_visible_child_name("profiles");
                }
                if let Some(f) = snapshot.frame {
                    let mut s = generation.borrow_mut();
                    if s.generation != Some(f.generation) {
                        let bytes = glib::Bytes::from_owned(f.rgba.clone());
                        let texture = gdk::MemoryTexture::new(
                            f.width as i32,
                            f.height as i32,
                            gdk::MemoryFormat::R8g8b8a8,
                            &bytes,
                            f.width * 4,
                        );
                        pic.set_paintable(Some(&texture));
                        s.dimensions = [f.width, f.height];
                        if !fit_update.is_active() {
                            pic.set_size_request(f.width as i32, f.height as i32);
                            pic.set_can_shrink(false);
                        }
                        s.generation = Some(f.generation);
                    }
                }
                if let Some(cursor) = snapshot.cursor {
                    let mut s = generation.borrow_mut();
                    if s.cursor_generation != Some(cursor.generation) {
                        let bytes = glib::Bytes::from_owned(cursor.rgba.clone());
                        let texture = gdk::MemoryTexture::new(
                            cursor.width as i32,
                            cursor.height as i32,
                            gdk::MemoryFormat::R8g8b8a8,
                            &bytes,
                            cursor.width * 4,
                        );
                        let shape = gdk::Cursor::from_texture(
                            &texture,
                            cursor.hotspot[0] as i32,
                            cursor.hotspot[1] as i32,
                            None,
                        );
                        pic.set_cursor(Some(&shape));
                        s.cursor_generation = Some(cursor.generation);
                    }
                } else {
                    pic.set_cursor_from_name(Some("default"));
                }
                send_update.set_visible(generation.borrow().model.draft.clipboard);
                let text = generation
                    .borrow()
                    .session
                    .as_ref()
                    .and_then(|s| s.take_clipboard());
                if let Some(text) = text {
                    pic.clipboard().set_text(&text);
                }
            }
            glib::ControlFlow::Continue
        });
    let source = Rc::new(RefCell::new(Some(source)));
    connect.connect_clicked({
        let state = state.clone();
        let read = read_form.clone();
        let password = password.clone();
        let wake = wake.clone();
        let status = status.clone();
        let error = error.clone();
        let window = window.clone();
        let full = fullscreen.clone();
        let show_password = show_password.clone();
        let form = form.clone();
        move |button| {
            read();
            show_password.set_active(false);
            release(&state);
            let mut s = state.borrow_mut();
            s.session = None;
            s.generation = None;
            s.cursor_generation = None;
            let mut secret = password.text().to_string();
            password.set_text("");
            let result = Session::connect(
                s.model.draft.clone(),
                std::mem::take(&mut secret),
                wake.clone(),
            );
            secret.zeroize();
            match result {
                Ok(session) => {
                    s.session = Some(session);
                    error.set_text("");
                    status.set_text("Connecting…");
                    button.set_sensitive(false);
                    form.set_sensitive(false);
                    if s.model.draft.fullscreen {
                        full.set_active(true);
                        window.fullscreen();
                    }
                }
                Err(e) => error.set_text(&e.to_string()),
            }
        }
    });
    disconnect.connect_clicked({
        let state = state.clone();
        let stack = stack.clone();
        let picture = picture.clone();
        let status = status.clone();
        let full = fullscreen.clone();
        let form = form.clone();
        let connect = connect.clone();
        let window = window.clone();
        move |_| {
            if viewer_only {
                window.close();
                return;
            }
            form.set_sensitive(true);
            connect.set_sensitive(true);
            release(&state);
            state.borrow_mut().session = None;
            state.borrow_mut().generation = None;
            picture.set_paintable(None::<&gdk::Paintable>);
            stack.set_visible_child_name("profiles");
            status.set_text("Disconnected");
            full.set_active(false);
        }
    });
    fullscreen.connect_toggled({
        let window = window.clone();
        let state = state.clone();
        move |b| {
            release(&state);
            if b.is_active() {
                window.fullscreen()
            } else {
                window.unfullscreen()
            }
        }
    });
    release_button.connect_clicked({
        let state = state.clone();
        move |_| release(&state)
    });
    fit.connect_toggled({
        let picture = picture.clone();
        let state = state.clone();
        move |b| {
            let s = state.borrow();
            if b.is_active() {
                picture.set_size_request(-1, -1);
                picture.set_can_shrink(true);
            } else {
                picture.set_size_request(s.dimensions[0] as i32, s.dimensions[1] as i32);
                picture.set_can_shrink(false);
            }
        }
    });
    let viewport: Rc<dyn Fn() -> Viewport> = Rc::new({
        let state = state.clone();
        let picture = picture.clone();
        let fit = fit.clone();
        move || {
            Viewport::fit(
                [picture.width() as f32, picture.height() as f32],
                state.borrow().dimensions,
                fit.is_active(),
            )
        }
    });
    let click = gtk::GestureClick::new();
    click.set_button(0);
    click.connect_pressed({
        let state = state.clone();
        let picture = picture.clone();
        let viewport = viewport.clone();
        move |g, _, x, y| {
            picture.grab_focus();

            let p = viewport().point(x as f32, y as f32);
            if let Some((x, y)) = p {
                let mut s = state.borrow_mut();
                s.captured = true;
                s.buttons |= match g.current_button() {
                    1 => 1,
                    2 => 2,
                    3 => 4,
                    _ => 0,
                };
                s.point = (x, y);
                let b = s.buttons;
                if let Some(c) = &mut s.session {
                    c.send(Input::Pointer(x, y, b));
                }
            }
        }
    });
    click.connect_released({
        let state = state.clone();
        move |g, _, _, _| {
            let mut s = state.borrow_mut();
            s.buttons &= !match g.current_button() {
                1 => 1,
                2 => 2,
                3 => 4,
                _ => 0,
            };
            let (x, y) = s.point;
            let b = s.buttons;
            if let Some(c) = &mut s.session {
                c.send(Input::Pointer(x, y, b));
            }
        }
    });
    click.connect_unpaired_release({
        let state = state.clone();
        move |_, _, _, button, _| {
            let mut s = state.borrow_mut();
            s.buttons &= !match button {
                1 => 1,
                2 => 2,
                3 => 4,
                _ => 0,
            };
            let (x, y) = s.point;
            let b = s.buttons;
            if let Some(c) = &mut s.session {
                c.send(Input::Pointer(x, y, b));
            }
        }
    });
    picture.add_controller(click);
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion({
        let state = state.clone();
        let viewport = viewport.clone();
        move |_, x, y| {
            let p = viewport().point(x as f32, y as f32);
            if let Some((x, y)) = p {
                let mut s = state.borrow_mut();
                if s.captured {
                    s.point = (x, y);
                    let b = s.buttons;
                    if let Some(c) = &mut s.session {
                        c.send(Input::Pointer(x, y, b));
                    }
                }
            }
        }
    });
    picture.add_controller(motion);
    let scroll_controller =
        gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    scroll_controller.connect_scroll({
        let state = state.clone();
        move |_, _, dy| {
            let mut s = state.borrow_mut();
            if !s.captured {
                return glib::Propagation::Proceed;
            }
            let (x, y) = s.point;
            let buttons = s.buttons;
            let b = if dy < 0. { 8 } else { 16 };
            if let Some(c) = &mut s.session {
                c.send(Input::Pointer(x, y, buttons | b));
                c.send(Input::Pointer(x, y, buttons));
            }
            glib::Propagation::Stop
        }
    });
    picture.add_controller(scroll_controller);
    let key = gtk::EventControllerKey::new();
    key.connect_key_pressed({
        let state = state.clone();
        let full = fullscreen.clone();
        move |_, key, hardware, mods| {
            if key == gdk::Key::Escape
                && mods.contains(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK)
            {
                release(&state);
                full.set_active(false);
                return glib::Propagation::Stop;
            }
            let mut s = state.borrow_mut();
            if !s.captured {
                return glib::Propagation::Proceed;
            }
            let code = glib::translate::IntoGlib::into_glib(key);
            let code = if s.model.draft.map_ctrl_to_command {
                match code {
                    0xffe3 => 0xffeb,
                    0xffe4 => 0xffec,
                    _ => code,
                }
            } else {
                code
            };
            s.keys.insert(hardware, code);
            if let Some(c) = &mut s.session {
                c.send(Input::Key(code, true));
            }
            glib::Propagation::Stop
        }
    });
    key.connect_key_released({
        let state = state.clone();
        move |_, _, hardware, _| {
            let mut s = state.borrow_mut();
            if let Some(code) = s.keys.remove(&hardware)
                && let Some(c) = &mut s.session
            {
                c.send(Input::Key(code, false));
            }
        }
    });
    picture.add_controller(key);
    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave({
        let state = state.clone();
        move |_| release(&state)
    });
    picture.add_controller(focus);
    send_clipboard.connect_clicked({
        let state = state.clone();
        let picture = picture.clone();
        move |_| {
            let state = state.clone();
            let session_id = state.borrow().session.as_ref().map(|s| s.id());
            picture
                .clipboard()
                .read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
                    if let Ok(Some(text)) = result
                        && let Some(c) = &mut state.borrow_mut().session
                        && Some(c.id()) == session_id
                    {
                        c.send(Input::Clipboard(text.to_string()));
                    }
                });
        }
    });
    window.connect_close_request({
        let state = state.clone();
        let source = source.clone();
        move |_| {
            release(&state);
            state.borrow_mut().session = None;
            if let Some(source) = source.borrow_mut().take() {
                source.remove();
            }
            glib::Propagation::Proceed
        }
    });
    let shortcut = gtk::EventControllerKey::new();
    shortcut.connect_key_pressed({
        let stack = stack.clone();
        let connect = connect.clone();
        move |_, key, _, mods| {
            if key == gdk::Key::Return
                && mods.contains(gdk::ModifierType::CONTROL_MASK)
                && stack.visible_child_name().as_deref() == Some("profiles")
            {
                connect.emit_clicked();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    });
    window.add_controller(shortcut);
    if viewer_only {
        stack.set_visible_child_name("remote");
        status.set_text("Connected · unencrypted VNC");
        if state.borrow().model.draft.fullscreen {
            fullscreen.set_active(true);
        }
    }
    window.present();
}
