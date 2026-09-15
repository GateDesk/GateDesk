//! C interface for behaviors support (a.k.a windowless controls).

#![allow(non_camel_case_types, non_snake_case)]
#![allow(dead_code)]

use capi::sctypes::*;
use capi::scdom::*;
use capi::scvalue::{VALUE};
use capi::scgraphics::{HGFX};
use capi::scom::{som_asset_t, som_passport_t};

#[repr(C)]
pub struct BEHAVIOR_EVENT_PARAMS
{
	/// Behavior event code. See [`BEHAVIOR_EVENTS`](enum.BEHAVIOR_EVENTS.html).
	pub cmd: UINT,

	/// Target element handler.
	pub heTarget: HELEMENT,

	/// Source element.
	pub he: HELEMENT,

	/// UI action causing change.
	pub reason: UINT_PTR,

	/// Auxiliary data accompanied with the event.
	pub data: VALUE,

	/// Name of the custom event (when `cmd` is [`BEHAVIOR_EVENTS::CUSTOM`](enum.BEHAVIOR_EVENTS.html#variant.CUSTOM)).
	/// Since 4.2.8.
	pub name: LPCWSTR,
}


#[repr(C)]
#[derive(Copy, Clone)]
#[derive(Debug, PartialOrd, PartialEq)]
pub enum INITIALIZATION_EVENTS
{
	BEHAVIOR_DETACH = 0,
	BEHAVIOR_ATTACH = 1,
}

#[repr(C)]
pub struct INITIALIZATION_PARAMS
{
	pub cmd: INITIALIZATION_EVENTS,
}

#[repr(C)]
#[derive(Copy, Clone)]
#[derive(Debug, PartialOrd, PartialEq)]
pub enum SOM_EVENTS
{
	SOM_GET_PASSPORT = 0,
	SOM_GET_ASSET = 1,
}

#[repr(C)]
pub union SOM_PARAMS_DATA
{
	pub asset: *const som_asset_t,
	pub passport: *const som_passport_t,
}

#[repr(C)]
pub struct SOM_PARAMS
{
	pub cmd: SOM_EVENTS,
	pub result: SOM_PARAMS_DATA,
}

/// Identifiers of methods currently supported by intrinsic behaviors.
///
/// Not an enum: Sciter also dispatches application-defined method ids (anything
/// at or above [`FIRST_APPLICATION_METHOD_ID`](#associatedconstant.FIRST_APPLICATION_METHOD_ID),
/// to say nothing of ids from future engine versions), and an enum cannot hold a
/// value that is not one of its variants. Decoding an id with `mem::transmute`
/// was undefined behaviour for such a value and is now a hard abort at runtime.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, Hash)]
pub struct BEHAVIOR_METHOD_IDENTIFIERS(pub UINT);

#[allow(non_upper_case_globals)]
impl BEHAVIOR_METHOD_IDENTIFIERS {
  /// Raise a click event.
  pub const DO_CLICK: Self = Self(1);

  /// `IS_EMPTY_PARAMS::is_empty` reflects the `:empty` state of the element.
  pub const IS_EMPTY: Self = Self(0xFC);

  /// `VALUE_PARAMS`
  pub const GET_VALUE: Self = Self(0xFD);
  /// `VALUE_PARAMS`
  pub const SET_VALUE: Self = Self(0xFE);

  /// User method identifier used in custom behaviors.
  ///
  /// All custom event codes shall be greater than this number.
  /// All codes below this will be used solely by application - Sciter will not intrepret it
  /// and will do just dispatching. To send event notifications with  these codes use
  /// `SciterCallBehaviorMethod` API.
  pub const FIRST_APPLICATION_METHOD_ID: Self = Self(0x100);
}

/// Method arguments used in `SciterCallBehaviorMethod()` or `HANDLE_METHOD_CALL`.
#[repr(C)]
pub struct METHOD_PARAMS {
  /// [`BEHAVIOR_METHOD_IDENTIFIERS`](enum.BEHAVIOR_METHOD_IDENTIFIERS.html) or user identifiers.
  pub method: UINT,
}

#[repr(C)]
pub struct IS_EMPTY_PARAMS {
  pub method: UINT,
  pub is_empty: UINT,
}

#[repr(C)]
pub struct VALUE_PARAMS {
  pub method: UINT,
  pub value: VALUE,
}

#[repr(C)]
pub struct SCRIPTING_METHOD_PARAMS
{
	pub name: LPCSTR,
	pub argv: *const VALUE,
	pub argc: UINT,
	pub result: VALUE,
}

#[repr(C)]
pub struct TIMER_PARAMS
{
	pub timerId: UINT_PTR,
}

#[repr(C)]
pub struct DRAW_PARAMS {
	/// Element layer to draw.
	pub layer: DRAW_EVENTS,

	/// Graphics context.
	pub gfx: HGFX,

	/// Element area.
	pub area: RECT,

	/// Zero at the moment.
	pub reserved: UINT,
}

/// Layer to draw.
#[repr(C)]
#[derive(Copy, Clone)]
#[derive(Debug, PartialEq)]
pub enum DRAW_EVENTS {
	DRAW_BACKGROUND = 0,
	DRAW_CONTENT,
	DRAW_FOREGROUND,
	/// Note: since 4.2.3.
	DRAW_OUTLINE,
}


/// Event groups for subscription.
///
/// A bit set, not an enum: the groups are combined with `|` (see
/// `default_events`), and an enum cannot hold a combination that is not one of
/// its variants. The previous enum-based version OR'ed through `mem::transmute`,
/// which is undefined behaviour for an unmapped discriminant and now aborts at
/// runtime.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, Hash)]
pub struct EVENT_GROUPS(pub UINT);

#[allow(non_upper_case_globals)]
impl EVENT_GROUPS
{ /// Attached/detached.
	pub const HANDLE_INITIALIZATION: Self = Self(0x0000);
	/// Mouse events.
	pub const HANDLE_MOUSE: Self = Self(0x0001);
	/// Key events.
	pub const HANDLE_KEY: Self = Self(0x0002);
	/// Focus events, if this flag is set it also means that element it attached to is focusable.
	pub const HANDLE_FOCUS: Self = Self(0x0004);
	/// Scroll events.
	pub const HANDLE_SCROLL: Self = Self(0x0008);
	/// Timer event.
	pub const HANDLE_TIMER: Self = Self(0x0010);
	/// Size changed event.
	pub const HANDLE_SIZE: Self = Self(0x0020);
	/// Drawing request (event).
	pub const HANDLE_DRAW: Self = Self(0x0040);
	/// Requested data has been delivered.
	pub const HANDLE_DATA_ARRIVED: Self = Self(0x080);

	/// Logical, synthetic events:
  /// `BUTTON_CLICK`, `HYPERLINK_CLICK`, etc.,
	/// a.k.a. notifications from intrinsic behaviors.
	pub const HANDLE_BEHAVIOR_EVENT: Self = Self(0x0100);
	/// Behavior specific methods.
	pub const HANDLE_METHOD_CALL: Self = Self(0x0200);
	/// Behavior specific methods.
	pub const HANDLE_SCRIPTING_METHOD_CALL: Self = Self(0x0400);

	/// Behavior specific methods using direct `tiscript::value`'s.
	#[deprecated(since="Sciter 4.4.3.24", note="TIScript native API is gone, use SOM instead.")]
	pub const HANDLE_TISCRIPT_METHOD_CALL: Self = Self(0x0800);

	/// System drag-n-drop.
	pub const HANDLE_EXCHANGE: Self = Self(0x1000);
	/// Touch input events.
	pub const HANDLE_GESTURE: Self = Self(0x2000);
	/// SOM passport and asset requests.
	pub const HANDLE_SOM: Self = Self(0x8000);

	/// All of them.
	pub const HANDLE_ALL: Self = Self(0xFFFF);

	/// Special value for getting subscription flags.
	pub const SUBSCRIPTIONS_REQUEST: Self = Self(UINT::MAX);
}

/// Event propagation schema.
///
/// A bit set, not an enum: `BUBBLING_HANDLED`/`SINKING_HANDLED` are `BUBBLING`/
/// `SINKING` with the "consumed" bit added, and Sciter may add further bits.
/// Decoding them with `mem::transmute` is undefined behaviour for an unmapped
/// value and now aborts at runtime.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, Hash)]
pub struct PHASE_MASK(pub UINT);

#[allow(non_upper_case_globals)]
impl PHASE_MASK
{
	/// Bubbling phase – direction: from a child element to all its containers.
	pub const BUBBLING: Self = Self(0);
	/// Sinking phase – direction: from containers to target child element.
	pub const SINKING: Self = Self(0x0_8000);
	/// Bubbling event consumed by some element.
	pub const BUBBLING_HANDLED: Self = Self(0x1_0000);
	/// Sinking event consumed by some child.
	pub const SINKING_HANDLED: Self = Self(0x1_8000);
}

#[repr(C)]
#[derive(Copy, Clone)]
#[derive(Debug, PartialOrd, PartialEq)]
/// Mouse buttons.
pub enum MOUSE_BUTTONS
{
	NONE = 0,

	/// Left button.
	MAIN = 1,
	/// Right button.
	PROP = 2,
	/// Middle button.
	MIDDLE = 3,
}

#[repr(C)]
#[derive(Copy, Clone)]
#[derive(Debug, Default, PartialOrd, PartialEq)]
/// Keyboard modifier buttons state.
pub struct KEYBOARD_STATES(u32);

impl KEYBOARD_STATES {
	pub const CONTROL_KEY_PRESSED: u32 = 0x01;
	pub const SHIFT_KEY_PRESSED: u32 = 0x02;
	pub const ALT_KEY_PRESSED: u32 = 0x04;
}

impl std::convert::From<u32> for KEYBOARD_STATES {
	fn from(u: u32) -> Self {
		Self(u)
	}
}

#[repr(C)]
#[derive(Copy, Clone)]
#[derive(Debug, PartialOrd, PartialEq)]
/// Keyboard input events.
pub enum KEY_EVENTS
{
	KEY_DOWN = 0,
	KEY_UP,
	KEY_CHAR,
}

#[repr(C)]
#[derive(Copy, Clone)]
#[derive(Debug, PartialOrd, PartialEq)]
/// Mouse events.
pub enum MOUSE_EVENTS
{
	MOUSE_ENTER = 0,
	MOUSE_LEAVE,
	MOUSE_MOVE,
	MOUSE_UP,
	MOUSE_DOWN,
	MOUSE_DCLICK,
	MOUSE_WHEEL,
	/// mouse pressed ticks
	MOUSE_TICK,
	/// mouse stay idle for some time
	MOUSE_IDLE,

	/// item dropped, target is that dropped item
	DROP        = 9,
	/// drag arrived to the target element that is one of current drop targets.
	DRAG_ENTER  = 0xA,
	/// drag left one of current drop targets. target is the drop target element.
	DRAG_LEAVE  = 0xB,
	/// drag src notification before drag start. To cancel - return true from handler.
	DRAG_REQUEST = 0xC,

	/// mouse triple click.
	MOUSE_TCLICK = 0xF,

	/// mouse click event
	MOUSE_CLICK = 0xFF,

	/// This flag is `OR`ed with `MOUSE_ENTER..MOUSE_DOWN` codes if dragging operation is in effect.
	/// E.g. event `DRAGGING | MOUSE_MOVE` is sent to underlying DOM elements while dragging.
	DRAGGING = 0x100,
}

/// General event source triggers
///
/// Not an enum: the reason of a `BEHAVIOR_EVENTS` notification is an opaque
/// engine-supplied number for every event that is not an edit or a video-bind
/// request, so a value outside this list is normal. Decoding it with
/// `mem::transmute` was undefined behaviour and now aborts at runtime.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, Hash)]
pub struct CLICK_REASON(pub UINT);

#[allow(missing_docs, non_upper_case_globals)]
impl CLICK_REASON
{
  /// By mouse button.
	pub const BY_MOUSE_CLICK: Self = Self(0);
  /// By keyboard (e.g. spacebar).
	pub const BY_KEY_CLICK: Self = Self(1);
  /// Synthesized, by code.
	pub const SYNTHESIZED: Self = Self(2);
  /// Icon click, e.g. arrow icon on drop-down select.
	pub const BY_MOUSE_ON_ICON: Self = Self(3);
}

/// Edit control change trigger.
///
/// Not an enum, for the same reason as [`CLICK_REASON`](struct.CLICK_REASON.html):
/// the engine may report a reason this crate does not know about, and decoding
/// it with `mem::transmute` was undefined behaviour.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, Hash)]
pub struct EDIT_CHANGED_REASON(pub UINT);

#[allow(non_upper_case_globals)]
impl EDIT_CHANGED_REASON
{
	/// Single char insertion.
	pub const BY_INS_CHAR: Self = Self(0);
	/// Character range insertion, clipboard.
	pub const BY_INS_CHARS: Self = Self(1);
	/// Single char deletion.
	pub const BY_DEL_CHAR: Self = Self(2);
	/// Character range (selection) deletion.
	pub const BY_DEL_CHARS: Self = Self(3);
	/// Undo/redo.
	pub const BY_UNDO_REDO: Self = Self(4);
	/// Single char insertion, previous character was inserted in previous position.
	pub const CHANGE_BY_INS_CONSECUTIVE_CHAR: Self = Self(5);
	/// Single char removal, previous character was removed in previous position
	pub const CHANGE_BY_DEL_CONSECUTIVE_CHAR: Self = Self(6);
	pub const CHANGE_BY_CODE: Self = Self(7);
}

/// Behavior event codes.
///
/// Not an enum: Sciter owns this code space, sends events this crate has no
/// variant for (`MEDIA_CHANGED`-style engine notifications, custom application
/// codes at or above [`FIRST_APPLICATION_EVENT_CODE`](#associatedconstant.FIRST_APPLICATION_EVENT_CODE),
/// and everything a future engine version adds), and an enum cannot hold a value
/// that is not one of its variants. Decoding `BEHAVIOR_EVENT_PARAMS::cmd` with
/// `mem::transmute` was undefined behaviour for such a value and now aborts at
/// runtime - which is exactly what a `<video>` notification did.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Eq, Hash)]
pub struct BEHAVIOR_EVENTS(pub UINT);

#[allow(non_upper_case_globals)]
impl BEHAVIOR_EVENTS
{
	/// click on button
	pub const BUTTON_CLICK: Self = Self(0x00);
	/// mouse down or key down in button
	pub const BUTTON_PRESS: Self = Self(0x01);
	/// checkbox/radio/slider changed its state/value
	pub const BUTTON_STATE_CHANGED: Self = Self(0x02);
	/// before text change
	pub const EDIT_VALUE_CHANGING: Self = Self(0x03);
	/// after text change
	pub const EDIT_VALUE_CHANGED: Self = Self(0x04);
	/// selection in `<select>` is changed
	pub const SELECT_SELECTION_CHANGED: Self = Self(0x05);
	// node in select expanded/collapsed, heTarget is the node - OBSOLETE since 4.4.4.9
	// SELECT_STATE_CHANGED = 0x06
	/// value of `<select>` is changed
	pub const SELECT_VALUE_CHANGED: Self = Self(0x06);

	/// request to show popup just received,
	///     here DOM of popup element can be modifed.
	pub const POPUP_REQUEST: Self = Self(0x07);

	/// popup element has been measured and ready to be shown on screen,
	///     here you can use functions like `ScrollToView`.
	pub const POPUP_READY: Self = Self(0x08);

	/// popup element is closed,
	///     here DOM of popup element can be modifed again - e.g. some items can be removed to free memory.
	pub const POPUP_DISMISSED: Self = Self(0x09);

	/// menu item activated by mouse hover or by keyboard,
	pub const MENU_ITEM_ACTIVE: Self = Self(0x0A);

	/// menu item click,
	///   BEHAVIOR_EVENT_PARAMS structure layout
	///   BEHAVIOR_EVENT_PARAMS.cmd - MENU_ITEM_CLICK/MENU_ITEM_ACTIVE
	///   BEHAVIOR_EVENT_PARAMS.heTarget - owner(anchor) of the menu
	///   BEHAVIOR_EVENT_PARAMS.he - the menu item, presumably `<li>` element
	///   BEHAVIOR_EVENT_PARAMS.reason - BY_MOUSE_CLICK | BY_KEY_CLICK
	pub const MENU_ITEM_CLICK: Self = Self(0x0B);







	/// "right-click", BEHAVIOR_EVENT_PARAMS::he is current popup menu `HELEMENT` being processed or `NULL`.
	/// application can provide its own `HELEMENT` here (if it is `NULL`) or modify current menu element.
	pub const CONTEXT_MENU_REQUEST: Self = Self(0x10);

	/// broadcast notification, sent to all elements of some container being shown or hidden
	pub const VISIUAL_STATUS_CHANGED: Self = Self(0x11);
	/// broadcast notification, sent to all elements of some container that got new value of `:disabled` state
	pub const DISABLED_STATUS_CHANGED: Self = Self(0x12);

	/// popup is about to be closed
	pub const POPUP_DISMISSING: Self = Self(0x13);

	/// content has been changed, is posted to the element that gets content changed,  reason is combination of `CONTENT_CHANGE_BITS`.
	/// `target == NULL` means the window got new document and this event is dispatched only to the window.
	pub const CONTENT_CHANGED: Self = Self(0x15);

	/// generic click
	pub const CLICK: Self = Self(0x16);
	/// generic change
	pub const CHANGE: Self = Self(0x17);

	/// media changed (screen resolution, number of displays, etc.)
	pub const MEDIA_CHANGED: Self = Self(0x18);
	/// input language has changed, data is iso lang-country string
	pub const INPUT_LANGUAGE_CHANGED: Self = Self(0x19);
	/// editable content has changed
	pub const CONTENT_MODIFIED: Self = Self(0x1A);
	/// a broadcast notification being posted to all elements of some container
	/// that changes its `:read-only` state.
	pub const READONLY_STATUS_CHANGED: Self = Self(0x1B);
	/// change in `aria-live="polite|assertive"`
	pub const ARIA_LIVE_AREA_CHANGED: Self = Self(0x1C);

	// "grey" event codes  - notfications from behaviors from this SDK
	/// hyperlink click
	pub const HYPERLINK_CLICK: Self = Self(0x80);

	pub const PASTE_TEXT: Self = Self(0x8E);
	pub const PASTE_HTML: Self = Self(0x8F);

	/// element was collapsed, so far only `behavior:tabs` is sending these two to the panels
	pub const ELEMENT_COLLAPSED: Self = Self(0x90);
	/// element was expanded,
	pub const ELEMENT_EXPANDED: Self = Self(0x91);

	/// activate (select) child,
	/// used, for example, by `accesskeys` behaviors to send activation request, e.g. tab on `behavior:tabs`.
	pub const ACTIVATE_CHILD: Self = Self(0x92);

	/// ui state changed, observers shall update their visual states.
	/// is sent, for example, by `behavior:richtext` when caret position/selection has changed.
	pub const UI_STATE_CHANGED: Self = Self(0x95);

	/// `behavior:form` detected submission event. `BEHAVIOR_EVENT_PARAMS::data` field contains data to be posted.
	/// `BEHAVIOR_EVENT_PARAMS::data` is of type `T_MAP` in this case key/value pairs of data that is about
	/// to be submitted. You can modify the data or discard submission by returning true from the handler.
	pub const FORM_SUBMIT: Self = Self(0x96);

	/// `behavior:form` detected reset event (from `button type=reset`). `BEHAVIOR_EVENT_PARAMS::data` field contains data to be reset.
	/// `BEHAVIOR_EVENT_PARAMS::data` is of type `T_MAP` in this case key/value pairs of data that is about
	/// to be rest. You can modify the data or discard reset by returning true from the handler.
	pub const FORM_RESET: Self = Self(0x97);

	/// document in `behavior:frame` or root document is complete.
	pub const DOCUMENT_COMPLETE: Self = Self(0x98);

	/// requests to `behavior:history` (commands)
	pub const HISTORY_PUSH: Self = Self(0x99);
	pub const HISTORY_DROP: Self = Self(0x9A);
	pub const HISTORY_PRIOR: Self = Self(0x9B);
	pub const HISTORY_NEXT: Self = Self(0x9C);
	/// `behavior:history` notification - history stack has changed
	pub const HISTORY_STATE_CHANGED: Self = Self(0x9D);

	/// close popup request,
	pub const CLOSE_POPUP: Self = Self(0x9E);
	/// request tooltip, `evt.source` <- is the tooltip element.
	pub const TOOLTIP_REQUEST: Self = Self(0x9F);

	/// animation started (`reason=1`) or ended(`reason=0`) on the element.
	pub const ANIMATION: Self = Self(0xA0);

	/// document created, script namespace initialized. `target` -> the document
	pub const DOCUMENT_CREATED: Self = Self(0xC0);
	/// document is about to be closed, to cancel closing do: `evt.data = sciter::Value("cancel")`;
	pub const DOCUMENT_CLOSE_REQUEST: Self = Self(0xC1);
	/// last notification before document removal from the DOM
	pub const DOCUMENT_CLOSE: Self = Self(0xC2);
	/// document has got DOM structure, styles and behaviors of DOM elements. Script loading run is complete at this moment.
	pub const DOCUMENT_READY: Self = Self(0xC3);
	/// document just finished parsing - has got DOM structure. This event is generated before the `DOCUMENT_READY`.
	/// Since 4.0.3.
	pub const DOCUMENT_PARSED: Self = Self(0xC4);

	/// `<video>` "ready" notification
	pub const VIDEO_INITIALIZED: Self = Self(0xD1);
	/// `<video>` playback started notification
	pub const VIDEO_STARTED: Self = Self(0xD2);
	/// `<video>` playback stoped/paused notification
	pub const VIDEO_STOPPED: Self = Self(0xD3);
	/// `<video>` request for frame source binding,
	///   If you want to provide your own video frames source for the given target `<video>` element do the following:
	///
	///   1. Handle and consume this `VIDEO_BIND_RQ` request
	///   2. You will receive second `VIDEO_BIND_RQ` request/event for the same `<video>` element
	///      but this time with the `reason` field set to an instance of `sciter::video_destination` interface.
	///   3. `add_ref()` it and store it, for example, in a worker thread producing video frames.
	///   4. call `sciter::video_destination::start_streaming(...)` providing needed parameters
	///      call `sciter::video_destination::render_frame(...)` as soon as they are available
	///      call `sciter::video_destination::stop_streaming()` to stop the rendering (a.k.a. end of movie reached)
	pub const VIDEO_BIND_RQ: Self = Self(0xD4);

	/// `behavior:pager` starts pagination
	pub const PAGINATION_STARTS: Self = Self(0xE0);
	/// `behavior:pager` paginated page no, reason -> page no
	pub const PAGINATION_PAGE: Self = Self(0xE1);
	/// `behavior:pager` end pagination, reason -> total pages
	pub const PAGINATION_ENDS: Self = Self(0xE2);

	/// event with custom name.
	/// Since 4.2.8.
	pub const CUSTOM: Self = Self(0xF0);

	/// SSX, delayed mount_component
	pub const MOUNT_COMPONENT: Self = Self(0xF1);

	/// all custom event codes shall be greater than this number. All codes below this will be used
	/// solely by application - Sciter will not intrepret it and will do just dispatching.
	/// To send event notifications with  these codes use `SciterSend`/`PostEvent` API.
	pub const FIRST_APPLICATION_EVENT_CODE: Self = Self(0x100);
}


impl ::std::ops::BitOr for EVENT_GROUPS {
	type Output = EVENT_GROUPS;
	fn bitor(self, rhs: Self::Output) -> Self::Output {
		Self(self.0 | rhs.0)
	}
}
