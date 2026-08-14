use gpui::{
    Anchor, Animation, AnimationExt as _, AppContext as _, Context, ElementId, Entity, FontWeight,
    InteractiveElement as _, MouseButton, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
    rems,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    dialog::Dialog,
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    progress::Progress,
    scroll::{Scrollbar, ScrollbarShow},
    v_flex,
};
use std::time::Duration;

#[derive(Clone)]
struct QuickCommandDialogInputs {
    name: Entity<InputState>,
    remark: Entity<InputState>,
    command: Entity<InputState>,
}

use rust_i18n::t;

use crate::{
    TinyShell,
    session::config::{AuthMethod, QuickCommand, QuickCommandCategory},
    system::format_bytes,
};

mod connections;
mod deletion;
mod quick_commands;
mod selector;
mod ssh;
mod transfers;
mod windows;
