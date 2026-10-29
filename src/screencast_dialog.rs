use crate::app::CosmicPortal;
use crate::fl;
use crate::wayland::{CaptureSource, WaylandHelper};
use crate::widget::keyboard_wrapper::KeyboardWrapper;
use crate::widget::lucide;
use ashpd::desktop::screencast::SourceType;
use ashpd::enumflags2::BitFlags;
use cosmic::iced::keyboard::Key;
use cosmic::iced::keyboard::key::Named;
use cosmic::iced::{self, window};
use cosmic::widget::icon;
use fde::IconSource;

use cosmic::desktop::fde;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    KeyboardInteractivity, Layer, destroy_layer_surface,
};
use cosmic::iced::runtime::platform_specific::wayland::layer_surface::SctkLayerSurfaceSettings;
use cosmic::widget::autosize;
use cosmic::{theme, widget};
use cosmic_client_toolkit::sctk::output::OutputInfo;
use cosmic_client_toolkit::toplevel_info::ToplevelInfo;
use fde::unicase::Ascii;
use fde::{DesktopEntry, get_languages_from_env};
use std::mem;
use std::sync::LazyLock;
use tokio::sync::mpsc;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1;
use zbus::zvariant;

pub static SCREENCAST_ID: LazyLock<window::Id> = LazyLock::new(window::Id::unique);
pub static SCREENCAST_WIDGET_ID: LazyLock<widget::Id> =
    LazyLock::new(|| widget::Id::new("screencast".to_string()));

/// Drawn for a toplevel that advertises no icon, or names one the icon theme
/// does not ship. Same glyph the Access dialog falls back to, in place of the
/// blank slot `as_cosmic_icon`'s `application-default` chain leaves behind.
const UNKNOWN_APP_ICON: &[u8] = include_bytes!("../res/icons/lucide/circle-help.svg");
/// Marks the selected toplevel; was a literal `✓`, drawn from whatever font
/// happened to cover U+2713.
const SELECTED_ICON: &[u8] = include_bytes!("../res/icons/lucide/check.svg");

/// Size of both glyphs in a toplevel row.
const TOPLEVEL_ICON_SIZE: u16 = 24;

/// The dialog's hero slot for the app that asked, as in the Access dialog: the
/// app's own icon fills it, the Lucide fallback draws at its native size.
const REQUESTER_ICON_SLOT: u16 = 64;
const LUCIDE_ICON_SIZE: u16 = 24;

pub async fn hide_screencast_prompt(
    subscription_tx: &mpsc::Sender<crate::subscription::Event>,
    session_handle: &zvariant::ObjectPath<'_>,
) {
    let _ = subscription_tx
        .send(crate::subscription::Event::CancelScreencast(
            session_handle.to_owned(),
        ))
        .await;
}

pub async fn show_screencast_prompt(
    subscription_tx: &mpsc::Sender<crate::subscription::Event>,
    session_handle: &zvariant::ObjectPath<'_>,
    app_id: String,
    multiple: bool,
    source_types: BitFlags<SourceType>,
    wayland_helper: &WaylandHelper,
) -> Option<CaptureSources> {
    let locales = get_languages_from_env();
    let desktop_entries = load_desktop_entries(&locales).await;

    let toplevels = wayland_helper
        .toplevels()
        .into_iter()
        .map(|info| {
            let icon = get_desktop_entry(&desktop_entries, &info.app_id)
                .and_then(|x| Some(x.icon()?.to_string()));
            (info, icon)
        })
        .collect();

    let mut outputs = Vec::new();
    for output in wayland_helper.outputs() {
        let Some(info) = wayland_helper.output_info(&output) else {
            continue;
        };
        let source = CaptureSource::Output(output.clone());
        let image = wayland_helper
            .capture_source_shm(source, false)
            .await
            .and_then(|image| image.image_transformed().ok())
            .map(|image| {
                widget::image::Handle::from_rgba(image.width(), image.height(), image.into_vec())
            });
        outputs.push((output, info, image));
    }

    // Order outputs by their position in the display arrangement
    outputs.sort_by_key(|(_, info, _)| info.logical_position.unwrap_or((i32::MAX, i32::MAX)));

    let requester = Requester::of(&desktop_entries, &app_id, &locales);

    let (tx, mut rx) = mpsc::channel(1);
    let args = Args {
        session_handle: session_handle.to_owned(),
        outputs,
        toplevels,
        multiple,
        source_types,
        requester,
        tx,
        capture_sources: Default::default(),
    };
    subscription_tx
        .send(crate::subscription::Event::Screencast(args))
        .await
        .unwrap();
    rx.recv().await.unwrap()
}

async fn load_desktop_entries(locales: &[String]) -> Vec<DesktopEntry> {
    let mut entries = Vec::new();
    for p in fde::Iter::new(fde::default_paths()) {
        if let Ok(data) = tokio::fs::read_to_string(&p).await
            && let Ok(entry) = DesktopEntry::from_str(&p, &data, Some(locales))
        {
            entries.push(entry.to_owned());
        }
    }
    entries
}

fn get_desktop_entry<'a>(entries: &'a [DesktopEntry], id: &str) -> Option<&'a DesktopEntry> {
    fde::find_app_by_id(entries, Ascii::new(id))
}

/// The app the portal frontend says is asking, as its desktop entry names it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Requester {
    name: Option<String>,
    icon: Option<String>,
}

impl Requester {
    /// The frontend accepts an app id only when `<id>.desktop` exists, so that
    /// entry is the app. `find_app_by_id` alone also matches an id's last
    /// dot-part, or an entry's `StartupWMClass` or `Name`, and could name the
    /// caller after another app; it stays the fallback for ids with no entry
    /// of their own. An empty id is no app.
    fn of(entries: &[DesktopEntry], app_id: &str, locales: &[String]) -> Self {
        if app_id.is_empty() {
            return Self::default();
        }
        let entry = entries
            .iter()
            .find(|entry| Ascii::new(entry.id()) == Ascii::new(app_id))
            .or_else(|| get_desktop_entry(entries, app_id));
        let Some(entry) = entry else {
            return Self::default();
        };
        Self {
            name: entry.name(locales).map(|name| name.into_owned()),
            icon: entry
                .icon()
                .filter(|icon| !icon.is_empty())
                .map(str::to_string),
        }
    }
}

/// The asking app's own icon, theme-resolved, or the Lucide fallback when it
/// names none or one the theme does not ship.
fn requester_icon(name: Option<&str>) -> cosmic::Element<'_, Msg> {
    let named = match name.map(IconSource::from_unknown) {
        Some(IconSource::Path(path)) => Some(icon::from_path(path)),
        Some(IconSource::Name(name)) => {
            let named = icon::from_name(name.as_str())
                .prefer_svg(true)
                .size(REQUESTER_ICON_SLOT);
            named.clone().path().map(|_| named.handle())
        }
        None => None,
    };
    match named {
        Some(handle) => icon::icon(handle).size(REQUESTER_ICON_SLOT).into(),
        None => widget::container(lucide::icon(UNKNOWN_APP_ICON, LUCIDE_ICON_SIZE))
            .center(iced::Length::Fixed(f32::from(REQUESTER_ICON_SLOT)))
            .into(),
    }
}

fn create_dialog() -> cosmic::Task<cosmic::Action<crate::app::Msg>> {
    cosmic::surface::surface_task::<crate::app::Msg>(cosmic::surface::action::simple_layer_shell::<
        crate::app::Msg,
    >(
        Default::default,
        move || SctkLayerSurfaceSettings {
            id: *SCREENCAST_ID,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            namespace: "screencast".into(),
            layer: Layer::Overlay,
            size: None,
            ..Default::default()
        },
        None::<fn() -> cosmic::Element<'static, cosmic::Action<crate::app::Msg>>>,
    ))
}

#[derive(Clone, Copy, Debug)]
enum Tab {
    Outputs,
    Windows,
}

#[derive(Debug, Clone)]
pub struct Args {
    session_handle: zvariant::ObjectPath<'static>,
    multiple: bool,
    source_types: BitFlags<SourceType>,
    outputs: Vec<(WlOutput, OutputInfo, Option<widget::image::Handle>)>,
    toplevels: Vec<(ToplevelInfo, Option<String>)>,
    requester: Requester,
    // Should be oneshot, but need `Clone` bound
    tx: mpsc::Sender<Option<CaptureSources>>,
    capture_sources: CaptureSources,
}

impl Args {
    fn send_response(self, response: Option<CaptureSources>) {
        tokio::spawn(async move {
            if let Err(err) = self.tx.send(response).await {
                tracing::error!("Failed to send screencast event: {}", err);
            }
        });
    }
}

// TODO order?
#[derive(Clone, Debug, Default)]
pub struct CaptureSources {
    pub outputs: Vec<WlOutput>,
    pub toplevels: Vec<ExtForeignToplevelHandleV1>,
}

impl CaptureSources {
    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty() && self.toplevels.is_empty()
    }

    pub fn clear(&mut self) {
        self.outputs.clear();
        self.toplevels.clear();
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    ActivateTab(widget::segmented_button::Entity),
    SelectOutput(WlOutput),
    SelectToplevel(ExtForeignToplevelHandleV1),
    Share,
    Cancel,
}

fn active_tab(portal: &CosmicPortal) -> Tab {
    *portal.screencast_tab_model.active_data::<Tab>().unwrap()
}

pub fn update_msg(
    portal: &mut CosmicPortal,
    msg: Msg,
) -> cosmic::Task<cosmic::Action<crate::app::Msg>> {
    let Some(args) = portal.screencast_args.as_mut() else {
        return cosmic::Task::none();
    };

    match msg {
        Msg::ActivateTab(tab) => {
            portal.screencast_tab_model.activate(tab);
        }
        Msg::SelectOutput(output) => {
            if let Some(idx) = args
                .capture_sources
                .outputs
                .iter()
                .position(|x| x == &output)
            {
                args.capture_sources.outputs.remove(idx);
            } else {
                if !args.multiple && !args.capture_sources.is_empty() {
                    args.capture_sources.clear();
                }
                args.capture_sources.outputs.push(output);
            }
        }
        Msg::SelectToplevel(toplevel) => {
            if let Some(idx) = args
                .capture_sources
                .toplevels
                .iter()
                .position(|t| t == &toplevel)
            {
                args.capture_sources.toplevels.remove(idx);
            } else {
                if !args.multiple && !args.capture_sources.is_empty() {
                    args.capture_sources.clear();
                }
                args.capture_sources.toplevels.push(toplevel);
            }
        }
        Msg::Share => {
            if let Some(mut args) = portal.screencast_args.take() {
                let response = mem::take(&mut args.capture_sources);
                args.send_response(Some(response));
                return destroy_layer_surface(*SCREENCAST_ID);
            }
        }
        Msg::Cancel => {
            if let Some(args) = portal.screencast_args.take() {
                args.send_response(None);
                return destroy_layer_surface(*SCREENCAST_ID);
            }
        }
    }
    cosmic::Task::none()
}

pub fn update_args(
    portal: &mut CosmicPortal,
    args: Args,
) -> cosmic::Task<cosmic::Action<crate::app::Msg>> {
    // If the dialog is already open, cancel previous request, but re-use dialog surface
    let command = if let Some(args) = portal.screencast_args.take() {
        args.send_response(None);
        cosmic::Task::none()
    } else {
        create_dialog()
    };

    portal.screencast_tab_model.clear();
    if args.source_types.contains(SourceType::Monitor) {
        portal
            .screencast_tab_model
            .insert()
            .data(Tab::Outputs)
            .text(fl!("output"));
    }
    if args.source_types.contains(SourceType::Window) {
        portal
            .screencast_tab_model
            .insert()
            .data(Tab::Windows)
            .text(fl!("window"));
    }
    portal.screencast_tab_model.activate_position(0);

    portal.screencast_args = Some(args);

    command
}

pub fn cancel(
    portal: &mut CosmicPortal,
    session_handle: zvariant::ObjectPath<'static>,
) -> cosmic::Task<cosmic::Action<crate::app::Msg>> {
    if portal
        .screencast_args
        .as_ref()
        .is_some_and(|args| args.session_handle == session_handle)
    {
        let args = portal.screencast_args.take().unwrap();
        args.send_response(None);
        destroy_layer_surface(*SCREENCAST_ID)
    } else {
        cosmic::Task::none()
    }
}

fn output_button_appearance(
    theme: &cosmic::Theme,
    is_active: bool,
    hovered: bool,
) -> widget::button::Style {
    let cosmic = theme.cosmic();
    let mut appearance = widget::button::Style::new();
    appearance.border_radius = cosmic.corner_radii.radius_s.into();
    if is_active {
        appearance.border_width = 2.0;
        appearance.border_color = cosmic.accent.base.into();
    }
    if hovered {
        appearance.background = Some(iced::Background::Color(cosmic.button.base.into()));
    }
    appearance
}

fn output_thumb_button<'a>(
    is_selected: bool,
    image_handle: Option<&'a widget::image::Handle>,
    width: f32,
    height: f32,
    msg: Msg,
) -> cosmic::Element<'a, Msg> {
    let content: cosmic::Element<'a, Msg> = match image_handle {
        Some(image_handle) => widget::image::Image::new(image_handle.clone())
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .content_fit(iced::ContentFit::Fill)
            .into(),
        None => widget::container(widget::text(""))
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .into(),
    };

    widget::button::custom(content)
        .width(iced::Length::Fixed(width))
        .height(iced::Length::Fixed(height))
        .padding(0)
        .selected(is_selected)
        .class(cosmic::theme::Button::Custom {
            active: Box::new(move |_focused, theme| {
                output_button_appearance(theme, is_selected, false)
            }),
            disabled: Box::new(|_theme| unreachable!()),
            hovered: Box::new(move |_focused, theme| {
                output_button_appearance(theme, is_selected, true)
            }),
            pressed: Box::new(move |_focused, theme| {
                output_button_appearance(theme, is_selected, true)
            }),
        })
        .on_press(msg)
        .into()
}

/// The toplevel's own icon stays theme-resolved — it is that app's branding —
/// but the misses are ours: `from_name` renders a silent blank when the theme
/// ships no such icon, and `as_cosmic_icon` would otherwise reach for the
/// theme's `application-default`. Resolve `path()` and draw Lucide on a miss.
fn toplevel_icon(name: Option<&str>) -> icon::Icon {
    let resolved = match name
        .filter(|name| !name.is_empty())
        .map(IconSource::from_unknown)
    {
        // `from_unknown` only yields `Path` for a file it already saw exist.
        Some(IconSource::Path(path)) => Some(icon::from_path(path)),
        Some(IconSource::Name(name)) => {
            // Look the art up at 128 as libcosmic's `as_cosmic_icon` did: the size
            // picks the theme directory, so asking at the drawn size would select
            // 24px raster art and upscale it on a HiDPI output.
            let named = icon::from_name(name.as_str()).prefer_svg(true).size(128);
            named.clone().path().map(|_| named.handle())
        }
        None => None,
    };

    match resolved {
        Some(handle) => icon::icon(handle).size(TOPLEVEL_ICON_SIZE),
        None => lucide::icon(UNKNOWN_APP_ICON, TOPLEVEL_ICON_SIZE),
    }
}

fn toplevel_button<'a>(
    label: &'a str,
    is_selected: bool,
    icon_name: Option<&str>,
    msg: Msg,
) -> cosmic::Element<'a, Msg> {
    let text = widget::text(label).class(theme::style::Text::Custom(|theme| {
        let container = theme.current_container();
        iced::core::widget::text::Style {
            color: Some(container.on.into()),
            ..Default::default()
        }
    }));
    let button = widget::button::custom(text)
        .width(iced::Length::Fill)
        .padding(0)
        // TODO hover style? Etc.
        // .style(theme::style::Button::Text)
        .class(theme::style::Button::Transparent)
        .selected(is_selected)
        .on_press(msg);
    let mut children = Vec::new();
    children.push(toplevel_icon(icon_name).into());
    children.push(button.into());
    if is_selected {
        children.push(lucide::icon(SELECTED_ICON, TOPLEVEL_ICON_SIZE).into());
    }
    widget::row::with_children(children).spacing(12).into()
}

pub(crate) fn view(portal: &CosmicPortal) -> cosmic::Element<'_, Msg> {
    let Some(args) = portal.screencast_args.as_ref() else {
        return widget::space::horizontal()
            .width(iced::Length::Fixed(1.0))
            .into();
    };
    let cancel_button = widget::button::standard(fl!("cancel")).on_press(Msg::Cancel);
    let mut share_button =
        widget::button::standard(fl!("share")).class(cosmic::style::Button::Suggested);
    if !args.capture_sources.is_empty() {
        share_button = share_button.on_press(Msg::Share);
    }

    let tabs =
        widget::tab_bar::horizontal(&portal.screencast_tab_model).on_activate(Msg::ActivateTab);

    let list: cosmic::Element<_> = match active_tab(portal) {
        Tab::Outputs => {
            // Position each output to match the display arrangement (as in the
            // cosmic-settings display page), scaled to fit the dialog.
            let geometry = |info: &OutputInfo| {
                let (x, y) = info.logical_position.unwrap_or((0, 0));
                let (w, h) = info.logical_size.unwrap_or((1920, 1080));
                (x, y, w.max(1), h.max(1))
            };

            let (mut min_x, mut min_y) = (i32::MAX, i32::MAX);
            let (mut max_x, mut max_y) = (i32::MIN, i32::MIN);
            for (_, info, _) in &args.outputs {
                let (x, y, w, h) = geometry(info);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + w);
                max_y = max_y.max(y + h);
            }
            let bbox_w = (max_x - min_x).max(1) as f32;
            let bbox_h = (max_y - min_y).max(1) as f32;

            // Scale the arrangement to fit a target area, and inset each region so
            // adjacent screens have a gap. The width is the dialog's beside the
            // asking app's icon and the dialog's gap after it.
            const DIALOG_W: f32 = 520.0;
            const TARGET_H: f32 = 320.0;
            const GAP: f32 = 6.0;
            let space_s = portal.core.system_theme().cosmic().spacing.space_s;
            let target_w = DIALOG_W - f32::from(REQUESTER_ICON_SLOT) - f32::from(space_s);
            let scale = (target_w / bbox_w).min(TARGET_H / bbox_h);

            let mut children = Vec::new();
            let mut regions = Vec::new();
            let mut labels = Vec::new();
            let mut selected = Vec::new();
            for (output, info, image) in &args.outputs {
                let (x, y, w, h) = geometry(info);
                let region = iced::core::Rectangle {
                    x: (x - min_x) as f32 * scale + GAP / 2.0,
                    y: (y - min_y) as f32 * scale + GAP / 2.0,
                    width: (w as f32 * scale - GAP).max(1.0),
                    height: (h as f32 * scale - GAP).max(1.0),
                };
                let is_selected = args.capture_sources.outputs.contains(output);
                children.push(output_thumb_button(
                    is_selected,
                    image.as_ref(),
                    region.width,
                    region.height,
                    Msg::SelectOutput(output.clone()),
                ));
                labels.push(info.name.clone().unwrap_or_default());
                selected.push(is_selected);
                regions.push(region);
            }

            let total = iced::core::Size::new(bbox_w * scale, bbox_h * scale);
            crate::widget::output_arrangement::OutputArrangement::new(
                children, regions, labels, selected, total,
            )
            .into()
        }
        Tab::Windows => {
            let mut list = widget::ListColumn::new();
            for (toplevel_info, icon) in &args.toplevels {
                let label = &toplevel_info.title;
                let is_selected = args
                    .capture_sources
                    .toplevels
                    .contains(&toplevel_info.foreign_toplevel);
                list = list.add(toplevel_button(
                    label,
                    is_selected,
                    icon.as_deref(),
                    Msg::SelectToplevel(toplevel_info.foreign_toplevel.clone()),
                ));
            }
            if args.toplevels.len() > 8 {
                widget::container(cosmic::widget::scrollable(list))
                    .max_height(380.)
                    .width(iced::Length::Fill)
                    .into()
            } else {
                list.into()
            }
        }
    };

    let unknown = fl!("unknown-application");
    let app_name = args.requester.name.as_deref().unwrap_or(&unknown);

    let control = widget::column::with_children(vec![tabs.into(), list]).spacing(8);
    autosize::autosize(
        KeyboardWrapper::new(
            widget::dialog()
                .title(fl!("share-screen"))
                // TODO adjust text for multiple select, types?
                .body(fl!("share-screen", "description", app_name = app_name))
                .icon(requester_icon(args.requester.icon.as_deref()))
                .secondary_action(cancel_button)
                .primary_action(share_button)
                .control(control),
            |key, _| match key {
                Key::Named(Named::Enter) => Some(Msg::Share),
                Key::Named(Named::Escape) => Some(Msg::Cancel),
                _ => None,
            },
        ),
        SCREENCAST_WIDGET_ID.clone(),
    )
    .max_width(572.)
    .max_height(884.)
    .min_width(1.)
    .min_height(1.)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, data: &str) -> DesktopEntry {
        DesktopEntry::from_str(path, data, None::<&[&str]>).unwrap()
    }

    /// The machine's Firefox, listed first as a user's own entries are.
    fn machine_firefox() -> DesktopEntry {
        entry(
            "/home/u/.local/share/applications/firefox.desktop",
            "[Desktop Entry]\nType=Application\nName=Firefox\nIcon=firefox\nExec=firefox\n",
        )
    }

    /// What kora-workspaces writes for an app it registers with the portal.
    fn registered(id: &str, name: &str, icon: &str) -> DesktopEntry {
        entry(
            &format!("/home/u/.local/share/applications/{id}.desktop"),
            &format!(
                "[Desktop Entry]\nType=Application\nName={name}\nNoDisplay=true\nIcon={icon}\n"
            ),
        )
    }

    #[test]
    fn the_picker_names_and_shows_the_app_the_portal_registered() {
        let id = "one.playtron.Workspace.work.firefox";
        let entries = [
            machine_firefox(),
            registered(id, "Firefox in Work", "firefox"),
        ];
        assert_eq!(
            Requester::of(&entries, id, &[]),
            Requester {
                name: Some("Firefox in Work".into()),
                icon: Some("firefox".into()),
            }
        );
    }

    /// An id's last dot-part names another app's entry: the id's own entry
    /// wins wherever it is listed.
    #[test]
    fn an_app_is_never_named_after_another_by_its_ids_last_part() {
        let id = "one.playtron.Workspace.work.com.example.firefox";
        let entries = [
            machine_firefox(),
            registered(id, "Example in Work", "example"),
        ];
        let requester = Requester::of(&entries, id, &[]);
        assert_eq!(requester.name.as_deref(), Some("Example in Work"));
        assert_eq!(requester.icon.as_deref(), Some("example"));
    }

    /// The frontend could not tell who is asking: no entry may answer for it,
    /// not even one whose empty `StartupWMClass` equals the empty id.
    #[test]
    fn no_app_id_names_no_app() {
        let entries = [entry(
            "/usr/share/applications/blank.desktop",
            "[Desktop Entry]\nType=Application\nName=Blank\nStartupWMClass=\nIcon=blank\n",
        )];
        assert_eq!(Requester::of(&entries, "", &[]), Requester::default());
    }

    /// A machine app whose id has no entry of its own keeps the loose match.
    #[test]
    fn a_machine_app_without_its_own_entry_is_found_by_its_last_part() {
        let entries = [machine_firefox()];
        assert_eq!(
            Requester::of(&entries, "org.mozilla.firefox", &[])
                .name
                .as_deref(),
            Some("Firefox")
        );
    }
}
