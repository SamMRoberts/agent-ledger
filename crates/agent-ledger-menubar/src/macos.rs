use std::{
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use agent_ledger_core::status::load_status_snapshot;
use anyhow::Result;
use tao::{
    event::{Event, StartCause},
    event_loop::{ControlFlow, EventLoopBuilder},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

use crate::{
    presentation::{present_error, present_status, IconState, PresentedStatus},
    Args,
};

enum UserEvent {
    RefreshTick,
    MenuEvent(MenuEvent),
}

struct AppState {
    root: PathBuf,
    tray_icon: Option<TrayIcon>,
    session_item: MenuItem,
    detail_item: MenuItem,
    tokens_item: MenuItem,
    integrity_item: MenuItem,
    refresh_item: MenuItem,
    open_item: MenuItem,
    quit_item: MenuItem,
    last_open_path: PathBuf,
}

impl AppState {
    fn new(root: PathBuf, _refresh_seconds: u64) -> Self {
        Self {
            last_open_path: root.join(".ledger"),
            root,
            tray_icon: None,
            session_item: MenuItem::new("Session: loading...", false, None),
            detail_item: MenuItem::new("Loading status...", false, None),
            tokens_item: MenuItem::new("Tokens: loading...", false, None),
            integrity_item: MenuItem::new("Integrity: loading...", false, None),
            refresh_item: MenuItem::new("Refresh now", true, None),
            open_item: MenuItem::new("Open .ledger folder", true, None),
            quit_item: MenuItem::new("Quit", true, None),
        }
    }

    fn build_menu(&self) -> Menu {
        let menu = Menu::new();
        menu.append_items(&[
            &self.session_item,
            &self.detail_item,
            &self.tokens_item,
            &self.integrity_item,
            &PredefinedMenuItem::separator(),
            &self.refresh_item,
            &self.open_item,
            &PredefinedMenuItem::separator(),
            &self.quit_item,
        ])
        .expect("append tray menu items");
        menu
    }

    fn initialize_tray(&mut self) -> Result<()> {
        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(self.build_menu()))
            .with_tooltip("agent-ledger menubar")
            .with_icon(icon_for_state(IconState::Idle)?)
            .with_title("AL idle")
            .build()?;
        self.tray_icon = Some(tray_icon);
        Ok(())
    }

    fn refresh(&mut self) -> Result<()> {
        match load_status_snapshot(&self.root) {
            Ok(snapshot) => {
                let presented = present_status(&self.root, snapshot.as_ref());
                self.last_open_path = snapshot
                    .as_ref()
                    .map(|status| status.session_dir.clone())
                    .unwrap_or_else(|| {
                        let ledger_dir = self.root.join(".ledger");
                        if ledger_dir.exists() {
                            ledger_dir
                        } else {
                            self.root.clone()
                        }
                    });
                self.apply(presented)?;
            }
            Err(error) => {
                self.last_open_path = self.root.clone();
                self.apply(present_error(&error.to_string()))?;
                eprintln!("agent-ledger menubar refresh failed: {error}");
            }
        }

        Ok(())
    }

    fn apply(&mut self, presented: PresentedStatus) -> Result<()> {
        self.session_item.set_text(&presented.session_label);
        self.detail_item.set_text(&presented.detail_label);
        self.tokens_item.set_text(&presented.tokens_label);
        self.integrity_item.set_text(&presented.integrity_label);
        self.open_item.set_text(&presented.open_label);

        if let Some(tray_icon) = &self.tray_icon {
            tray_icon.set_title(Some(&presented.title));
            tray_icon.set_tooltip(Some(&presented.tooltip))?;
            tray_icon.set_icon(Some(icon_for_state(presented.icon_state)?))?;
        }

        Ok(())
    }
}

pub fn run(args: Args) -> Result<()> {
    let root = normalize_root(&args.root)?;
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::MenuEvent(event));
    }));

    let refresh_proxy = event_loop.create_proxy();
    let refresh_seconds = args.refresh_seconds;
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(refresh_seconds));
        if refresh_proxy.send_event(UserEvent::RefreshTick).is_err() {
            break;
        }
    });

    let mut app = AppState::new(root, args.refresh_seconds);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                if let Err(error) = app.initialize_tray().and_then(|_| app.refresh()) {
                    eprintln!("agent-ledger menubar initialization failed: {error}");
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::RefreshTick) => {
                if let Err(error) = app.refresh() {
                    eprintln!("agent-ledger menubar refresh failed: {error}");
                }
            }
            Event::UserEvent(UserEvent::MenuEvent(event)) => {
                if event.id == app.refresh_item.id() {
                    if let Err(error) = app.refresh() {
                        eprintln!("agent-ledger menubar refresh failed: {error}");
                    }
                } else if event.id == app.open_item.id() {
                    if let Err(error) = open_path(&app.last_open_path) {
                        eprintln!("agent-ledger menubar open failed: {error}");
                    }
                } else if event.id == app.quit_item.id() {
                    app.tray_icon.take();
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    });
}

fn normalize_root(root: &Path) -> Result<PathBuf> {
    if root.exists() {
        Ok(root.canonicalize()?)
    } else {
        anyhow::bail!("root path does not exist: {}", root.display())
    }
}

fn open_path(path: &Path) -> Result<()> {
    let status = Command::new("open").arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("open command failed for {}", path.display())
    }
}

fn icon_for_state(state: IconState) -> Result<Icon> {
    let color = match state {
        IconState::Idle => [120, 120, 120, 255],
        IconState::Active => [52, 199, 89, 255],
        IconState::Finished => [10, 132, 255, 255],
        IconState::Failed | IconState::Error => [255, 69, 58, 255],
    };
    let size = 18;
    let mut rgba = vec![0_u8; size * size * 4];
    let center = (size as f32 - 1.0) / 2.0;
    let radius = 5.5_f32;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if dx * dx + dy * dy <= radius * radius {
                let index = (y * size + x) * 4;
                rgba[index..index + 4].copy_from_slice(&color);
            }
        }
    }

    Ok(Icon::from_rgba(rgba, size as u32, size as u32)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_icon_uses_valid_rgba_buffer() {
        icon_for_state(IconState::Active).expect("icon should generate");
    }
}
