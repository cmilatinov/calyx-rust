//! Reflection-driven drawing of one component in the inspector.
//!
//! A component is drawn under a collapsing header with its type inspector,
//! or, without one, field by field from its registered type info: nested
//! structs, lists and the fields' own type inspectors, with fields that name
//! a `group` gathered into collapsible sections. An enum with only unit
//! variants and no type inspector is a dropdown of its variants. A component that can be
//! disabled has a checkbox left of its title, and its body is greyed out and
//! read-only while it is off. The header's context menu offers removing the
//! component; the caller applies that and the checkbox.

use calyx_engine::component::{ComponentID, ComponentTransform};
use calyx_engine::reflect::type_registry::TypeRegistry;
use calyx_engine::reflect::{
    AttributeValue, EnumInfo, EnumVariantInfo, ListInfo, NamedField, Reflect, ReflectDefault,
    ReflectList, StructInfo, TypeInfo,
};
use calyx_engine::utils::TypeUuid;
use convert_case::{Case, Casing};
use egui::{Id, Rect, Response, RichText, TextStyle, Ui, Vec2, WidgetInfo, WidgetType};
use re_ui::list_item::{LabelContent, ListItem, PropertyContent, ShowCollapsingResponse};
use re_ui::UiExt;
use std::collections::HashMap;
use uuid::Uuid;

use super::inspector_registry::InspectorRegistry;
use super::pinned_components::{PinEdit, PinState};
use super::type_inspector::{InspectorContext, InspectorResponse};
use super::widgets::Widgets;
use crate::icons;
use crate::widgets::{List, ListContainer, ListItemContext};

/// Draws the checkbox of a component header in `slot`, the header's icon
/// slot. The box is as tall as the header's text and centred in the slot, and
/// it keeps its size while hovered, so it never grows past the slot into the
/// arrow or the title. Its click area is the box.
fn header_checkbox(ui: &mut Ui, slot: Rect, checked: &mut bool) -> Response {
    ui.scope(|ui| {
        let size = TextStyle::Body.resolve(ui.style()).size.min(slot.height());
        let spacing = ui.spacing_mut();
        spacing.icon_width_inner *= size / spacing.icon_width;
        spacing.icon_width = size;
        spacing.interact_size.y = size;
        let rect = Rect::from_center_size(slot.center(), Vec2::splat(size));
        ui.put(rect, |ui: &mut Ui| ui.re_checkbox(checked, ""))
    })
    .inner
}

/// A structural change the inspector made to a reflected list.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ListEdit {
    /// Appended a default element.
    Add,
    /// Removed the element at this index.
    Remove(usize),
    /// Moved the element at `from` so it ended up at `to`.
    Move { from: usize, to: usize },
}

/// Adapts a reflected list to the [`List`] widget.
///
/// Adding appends the element type's registered default, and each
/// structural change is logged in `edits`, so the inspector reports it as a
/// discrete edit: one undo step per add, remove or move.
struct ReflectListContainer<'a> {
    list: &'a mut dyn ReflectList,
    default: Option<&'a ReflectDefault>,
    edits: Vec<ListEdit>,
}

impl<'a> ReflectListContainer<'a> {
    /// Wraps `list`. Without `info` the element type is unknown, so the list
    /// cannot grow.
    fn new(
        list: &'a mut dyn ReflectList,
        type_registry: &'a TypeRegistry,
        info: Option<&ListInfo>,
    ) -> Self {
        let default = info
            .and_then(|info| type_registry.trait_meta::<ReflectDefault>(info.element.type_uuid));
        Self {
            list,
            default,
            edits: Vec::new(),
        }
    }
}

impl ListContainer for ReflectListContainer<'_> {
    type Item = dyn Reflect;

    fn len(&self) -> usize {
        self.list.len()
    }

    fn nth(&self, index: usize) -> &Self::Item {
        self.list.element(index).expect("list index in range")
    }

    fn nth_mut(&mut self, index: usize) -> &mut Self::Item {
        self.list.element_mut(index).expect("list index in range")
    }

    fn can_add(&self) -> bool {
        self.default.is_some()
    }

    fn push_default(&mut self) {
        let Some(default) = self.default else {
            return;
        };
        if self.list.push(default.default()).is_ok() {
            self.edits.push(ListEdit::Add);
        }
    }

    fn remove(&mut self, index: usize) {
        if self.list.remove(index).is_some() {
            self.edits.push(ListEdit::Remove(index));
        }
    }

    fn move_item(&mut self, from: usize, to: usize) {
        let Some(item) = self.list.remove(from) else {
            return;
        };
        match self.list.insert(to, item) {
            Ok(()) => self.edits.push(ListEdit::Move { from, to }),
            Err(item) => {
                // Out of range: put the element back where it was.
                let _ = self.list.insert(from, item);
            }
        }
    }
}

/// Where a value sits in its component's serialized JSON.
struct FieldTrail {
    /// Identifies the component being inspected.
    base: Id,
    path: Vec<String>,
}

impl FieldTrail {
    /// The trail of a component's root value. `base` identifies the
    /// component, such as its game object and type.
    fn root(base: Id) -> Self {
        Self {
            base,
            path: Vec::new(),
        }
    }

    fn child(&self, segment: impl ToString) -> Self {
        let mut path = self.path.clone();
        path.push(segment.to_string());
        Self {
            base: self.base,
            path,
        }
    }

    /// A widget id unique to this value: its component and its full path,
    /// list indices included, so a field repeated in every element of a
    /// list gets its own collapse, selection and drag state per element.
    fn id(&self) -> Id {
        self.base.with(&self.path)
    }
}

/// How a list's elements are labelled: `"{prefix} {index + base}"`.
///
/// A list field picks its own with the `element_label` and `element_base`
/// reflect attributes, as the Outline effect's styles do
/// (`#[reflect_attr(element_label = "Style", element_base = 1)]`) to match
/// the 1-based `outline_style` materials pick them by; any other list shows
/// "Element 0", "Element 1", and so on.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ElementLabel {
    prefix: &'static str,
    base: usize,
}

impl Default for ElementLabel {
    fn default() -> Self {
        Self {
            prefix: "Element",
            base: 0,
        }
    }
}

impl ElementLabel {
    /// The labels `field`'s attributes ask for, or the default.
    fn of_field(field: &NamedField) -> Self {
        let mut label = Self::default();
        if let Some(AttributeValue::String(prefix)) = field.attrs.get("element_label") {
            label.prefix = prefix;
        }
        if let Some(AttributeValue::Integer(base)) = field.attrs.get("element_base") {
            label.base = (*base).max(0) as usize;
        }
        label
    }

    /// The label of the element at `index`.
    fn text(self, index: usize) -> String {
        format!("{} {}", self.prefix, index + self.base)
    }
}

/// How the inspector draws one list element.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ElementView {
    /// A type inspector drawn inline on the element's row.
    Inline,
    /// A type inspector that draws its own rows under the element: an
    /// enum's, or one whose [`draws_own_rows`](super::type_inspector::TypeInspector::draws_own_rows) is true.
    Enum,
    /// A struct's fields, under the element.
    Struct,
    /// A nested list's elements, under the element.
    List,
    /// A dropdown of an enum's unit variants, inline on the element's row.
    UnitEnum,
    /// The element's type name, read-only.
    Label,
}

/// Picks how to draw a list element from its type inspector, whether that
/// inspector draws its own rows, its registered type info and whether it is
/// itself a list.
fn element_view(
    has_inspector: bool,
    own_rows: bool,
    info: Option<&TypeInfo>,
    is_list: bool,
) -> ElementView {
    match info {
        _ if has_inspector && own_rows => ElementView::Enum,
        Some(TypeInfo::Enum(_)) if has_inspector => ElementView::Enum,
        _ if has_inspector => ElementView::Inline,
        Some(TypeInfo::Struct(_)) => ElementView::Struct,
        Some(TypeInfo::Enum(info)) if info.is_unit_only() => ElementView::UnitEnum,
        _ if is_list => ElementView::List,
        _ => ElementView::Label,
    }
}

/// One entry of a struct's field list as the inspector lays it out.
enum FieldSection<'a> {
    /// A field outside any group.
    Field(&'a NamedField),
    /// A group's fields, in declaration order.
    Group(&'static str, Vec<&'a NamedField>),
}

/// Lays out `info`'s fields in declaration order, each group gathered into
/// one section at the position of its first field.
fn field_sections(info: &StructInfo) -> Vec<FieldSection<'_>> {
    let mut sections = Vec::new();
    let mut groups = HashMap::new();
    for field in info.fields.values() {
        let Some(group) = field.group() else {
            sections.push(FieldSection::Field(field));
            continue;
        };
        match groups.get(group) {
            Some(&index) => {
                if let FieldSection::Group(_, fields) = &mut sections[index] {
                    fields.push(field);
                }
            }
            None => {
                groups.insert(group, sections.len());
                sections.push(FieldSection::Group(group, vec![field]));
            }
        }
    }
    sections
}

/// One frame of a component drawn by [`ComponentInspector::show`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentResponse {
    /// What the component's widgets did to it.
    pub edit: InspectorResponse,
    /// Whether the header's context menu asked to remove the component.
    pub remove: bool,
    /// The pin edit the header's context menu asked for.
    pub pin: Option<PinEdit>,
    /// The enabled state the header's checkbox was switched to.
    pub enabled: Option<bool>,
}

/// Draws components with the type inspectors of `registry`, falling back to
/// their reflected fields.
#[derive(Copy, Clone)]
pub struct ComponentInspector<'a> {
    registry: &'a InspectorRegistry,
}

impl<'a> ComponentInspector<'a> {
    pub fn new(registry: &'a InspectorRegistry) -> Self {
        Self { registry }
    }

    /// Draws `instance` under a collapsing header, marked with a pin icon
    /// when `pin` says it is pinned. `enabled` is the component's enabled
    /// state, or `None` for a component that cannot be disabled: the header
    /// then has no checkbox, and the pin icon takes the icon slot the
    /// checkbox would; otherwise the pin sits right of the title. While the component is disabled, its body is
    /// greyed out and cannot be edited. The header's context menu offers the
    /// pin edits `pin` allows and removing it, except for the components
    /// every object keeps.
    pub fn show(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        pin: PinState,
        enabled: Option<bool>,
    ) -> ComponentResponse {
        self.show_with_pin(ui, ctx, instance, Some(pin), enabled)
    }

    /// Draws `instance` as [`Self::show`] does, with no pin icon or pin
    /// edits: for a value that is not one of an object's components, such
    /// as an effect entry of a post-processing profile.
    pub fn show_unpinned(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        enabled: Option<bool>,
    ) -> ComponentResponse {
        self.show_with_pin(ui, ctx, instance, None, enabled)
    }

    fn show_with_pin(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        pin: Option<PinState>,
        enabled: Option<bool>,
    ) -> ComponentResponse {
        let pinned = pin.is_some_and(|pin| pin.pinned);
        let name = display_name(&ctx.assets.types.read(), instance);
        let id = ui.make_persistent_id(name);
        let type_uuid = instance.uuid();
        let mut response = ComponentResponse::default();
        let mut toggled = None;
        let mut label = LabelContent::new(name).truncate(true);
        if let Some(enabled) = enabled {
            // The checkbox sits on top of the header row, so a click on it
            // reaches the checkbox and leaves the header as it is.
            let toggled = &mut toggled;
            label =
                label.with_icon_fn(move |ui, rect, _| {
                    let mut checked = enabled;
                    let checkbox = header_checkbox(ui, rect, &mut checked)
                        .on_hover_text(if enabled { "Disable" } else { "Enable" });
                    if checkbox.changed() {
                        *toggled = Some(checked);
                    }
                });
        }
        if pinned && enabled.is_none() {
            label = label.with_icon(&icons::PIN);
        } else if pinned {
            // The checkbox takes the icon slot, so the pin moves right.
            label = label
                .with_buttons(|ui| {
                    ui.add(
                        icons::PIN
                            .as_image()
                            .fit_to_exact_size(re_ui::DesignTokens::small_icon_size()),
                    )
                })
                .always_show_buttons(true);
        }
        let body_enabled = enabled.unwrap_or(true);
        let res = ListItem::new()
            .interactive(true)
            .force_background(re_ui::design_tokens().section_collapsing_header_color())
            .show_hierarchical_with_children_unindented(ui, id, true, label, |ui| {
                ui.add_enabled_ui(body_enabled, |ui| {
                    response.edit |= match self.registry.type_inspector_lookup(type_uuid) {
                        Some(inspector) => inspector.show_inspector(ui, ctx, instance),
                        None => self.show_fields(
                            ui,
                            ctx,
                            instance,
                            &FieldTrail::root(Id::new((ctx.game_object.node, type_uuid))),
                        ),
                    };
                });
            })
            .item_response;
        response.enabled = toggled;
        if res.clicked() {
            if let Some(mut state) = egui::collapsing_header::CollapsingState::load(ui.ctx(), id) {
                state.toggle(ui);
                state.store(ui.ctx());
            }
        }
        res.context_menu(|ui| {
            if let Some(pin) = pin {
                response.pin = Self::pin_menu(ui, pin);
            }
            let removable = type_uuid != ComponentID::type_uuid()
                && type_uuid != ComponentTransform::type_uuid();
            if removable && ui.button("Remove").clicked() {
                response.remove = true;
                ui.close_menu();
            }
            if let Some(inspector) = self.registry.type_inspector_lookup(type_uuid) {
                response.edit |= inspector.show_inspector_context(ui, ctx, instance);
            }
        });
        response
    }

    /// Draws the pin edits `pin` allows and returns the one clicked.
    fn pin_menu(ui: &mut Ui, pin: PinState) -> Option<PinEdit> {
        let mut edits = Vec::new();
        if pin.pinned {
            edits.push(("Unpin", PinEdit::Unpin));
            if pin.can_move_up {
                edits.push(("Move up", PinEdit::MoveUp));
            }
            if pin.can_move_down {
                edits.push(("Move down", PinEdit::MoveDown));
            }
        } else {
            edits.push(("Pin to top", PinEdit::Pin));
        }
        let mut clicked = None;
        for (label, edit) in edits {
            if ui.button(label).clicked() {
                clicked = Some(edit);
                ui.close_menu();
            }
        }
        clicked
    }

    /// Draws `instance`'s fields from its registered struct info, or nothing
    /// for a type that is not a registered struct.
    fn show_fields(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let type_registry = ctx.assets.types.read();
        match type_registry.type_info_by_id(instance.uuid()) {
            Some(TypeInfo::Struct(info)) => self.show_struct(ui, ctx, instance, info, trail),
            _ => InspectorResponse::NONE,
        }
    }

    fn show_struct(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        info: &StructInfo,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let mut response = InspectorResponse::NONE;
        for section in field_sections(info) {
            match section {
                FieldSection::Field(field) => {
                    response |= self.show_named_field(ui, ctx, instance, field, trail);
                }
                FieldSection::Group(group, fields) => {
                    let id = trail.id().with(("group", group));
                    let header = ListItem::new()
                        .interactive(true)
                        .show_hierarchical_with_children(
                            ui,
                            id,
                            true,
                            LabelContent::new(RichText::new(group).strong()),
                            |ui| {
                                for field in fields {
                                    response |=
                                        self.show_named_field(ui, ctx, instance, field, trail);
                                }
                            },
                        )
                        .item_response;
                    // The whole header row toggles the section, as a
                    // component's header does.
                    if header.clicked() {
                        if let Some(mut state) =
                            egui::collapsing_header::CollapsingState::load(ui.ctx(), id)
                        {
                            state.toggle(ui);
                            state.store(ui.ctx());
                        }
                    }
                }
            }
        }
        response
    }

    /// Draws `field` of `instance`, which `ctx` describes.
    fn show_named_field(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
        field: &NamedField,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let mut field_ctx = *ctx;
        field_ctx.field_name = Some(field.name);
        match field.get_reflect_mut(instance.as_reflect_mut()) {
            Some(value) => self.show_field(ui, &field_ctx, field, value, &trail.child(field.name)),
            None => InspectorResponse::NONE,
        }
    }

    fn show_field(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        field: &NamedField,
        instance: &mut dyn Reflect,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let mut name = field_display_name(field);
        name.push(' ');
        let type_registry = ctx.assets.types.read();
        if let Some(inspector) = self.registry.type_inspector_lookup(instance.uuid()) {
            if inspector.draws_own_rows()
                || matches!(
                    type_registry.type_info_by_id(instance.uuid()),
                    Some(TypeInfo::Enum(_))
                )
            {
                return inspector.show_inspector(ui, ctx, instance);
            }
            let mut response = InspectorResponse::NONE;
            Widgets::inspector_prop_value(ui, name, |ui, _| {
                response = inspector.show_inspector(ui, ctx, instance);
            });
            return response;
        }
        let id = trail.id();
        match type_registry.type_info_by_id(instance.uuid()) {
            Some(TypeInfo::Struct(info)) => {
                let mut nested_ctx = *ctx;
                nested_ctx.field_name = None;
                nested_ctx.type_info = info;
                let mut response = InspectorResponse::NONE;
                ListItem::new()
                    .interactive(false)
                    .show_hierarchical_with_children(
                        ui,
                        id,
                        true,
                        PropertyContent::new(name).show_only_when_collapsed(false),
                        |ui| {
                            response = self.show_struct(ui, &nested_ctx, instance, info, trail);
                        },
                    );
                response
            }
            Some(TypeInfo::List(info)) => self.show_list(
                ui,
                ctx,
                &type_registry,
                name.trim_end(),
                id,
                instance,
                Some(info),
                ElementLabel::of_field(field),
                trail,
            ),
            Some(TypeInfo::Enum(info)) if info.is_unit_only() && instance.as_enum().is_some() => {
                let label = name.trim_end().to_owned();
                let mut response = InspectorResponse::NONE;
                Widgets::inspector_prop_value(ui, name, |ui, _| {
                    response = unit_enum_dropdown(ui, id, &label, info, instance);
                });
                response
            }
            Some(_) => {
                Widgets::inspector_prop_value(ui, name, |ui, _| {
                    ui.label(instance.type_name_short());
                });
                InspectorResponse::NONE
            }
            None => InspectorResponse::NONE,
        }
    }

    /// Shows a list with the editor's [`List`] widget: a collapsing header
    /// with the element count and add/remove buttons, selectable elements
    /// labelled by `labels`, and drag-and-drop reordering. An add, remove or move is reported as a
    /// discrete change, so it is one undo step; element edits report as their
    /// inspectors do.
    #[allow(clippy::too_many_arguments)]
    fn show_list(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        type_registry: &TypeRegistry,
        name: &str,
        id: Id,
        instance: &mut dyn Reflect,
        info: Option<&ListInfo>,
        labels: ElementLabel,
        trail: &FieldTrail,
    ) -> InspectorResponse {
        let Some(list) = instance.as_list_mut() else {
            return InspectorResponse::NONE;
        };
        let mut container = ReflectListContainer::new(list, type_registry, info);
        let mut response = InspectorResponse::NONE;
        List::new(name, &mut container, |_| ()).id(id).show(
            ui,
            |ui,
             ListItemContext {
                 value,
                 index,
                 selected,
             }| {
                let (element, shown) = ui
                    .push_id(index, |ui| {
                        self.show_list_element(
                            ui,
                            ctx,
                            type_registry,
                            id.with(index),
                            labels.text(index),
                            selected,
                            value,
                            &trail.child(index),
                        )
                    })
                    .inner;
                response |= element;
                shown
            },
        );
        if !container.edits.is_empty() {
            response |= InspectorResponse::CHANGED;
        }
        response
    }

    /// Shows one list element as a selectable, draggable row.
    #[allow(clippy::too_many_arguments)]
    fn show_list_element(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        type_registry: &TypeRegistry,
        id: Id,
        label: String,
        selected: bool,
        element: &mut dyn Reflect,
        trail: &FieldTrail,
    ) -> (InspectorResponse, ShowCollapsingResponse<()>) {
        let element_info = type_registry.type_info_by_id(element.uuid());
        let inspector = self.registry.type_inspector_lookup(element.uuid());
        let view = element_view(
            inspector.is_some(),
            inspector.is_some_and(|inspector| inspector.draws_own_rows()),
            element_info,
            element.as_list().is_some(),
        );
        let item = ListItem::new()
            .selected(selected)
            .interactive(true)
            .draggable(true);
        let mut response = InspectorResponse::NONE;
        if let (ElementView::UnitEnum, Some(TypeInfo::Enum(info))) = (view, element_info) {
            let item_response = item.show_flat(
                ui,
                PropertyContent::new(label.as_str()).value_fn(|ui, _| {
                    response = unit_enum_dropdown(ui, id, &label, info, element);
                }),
            );
            let shown = ShowCollapsingResponse {
                item_response,
                body_response: None,
                openness: 0.0,
            };
            return (response, shown);
        }
        if let (ElementView::Inline, Some(inspector)) = (view, inspector) {
            let item_response = item.show_flat(
                ui,
                PropertyContent::new(label)
                    .value_fn(|ui, _| response = inspector.show_inspector(ui, ctx, element)),
            );
            let shown = ShowCollapsingResponse {
                item_response,
                body_response: None,
                openness: 0.0,
            };
            return (response, shown);
        }
        let shown = item.show_hierarchical_with_children(
            ui,
            id,
            true,
            PropertyContent::new(label).show_only_when_collapsed(false),
            |ui| match (view, inspector, element_info) {
                (ElementView::Enum, Some(inspector), _) => {
                    response = inspector.show_inspector(ui, ctx, element);
                }
                (ElementView::Struct, _, Some(TypeInfo::Struct(info))) => {
                    let mut nested_ctx = *ctx;
                    nested_ctx.field_name = None;
                    nested_ctx.type_info = info;
                    response = self.show_struct(ui, &nested_ctx, element, info, trail);
                }
                (ElementView::List, _, info) => {
                    let info = match info {
                        Some(TypeInfo::List(info)) => Some(info),
                        _ => None,
                    };
                    response = self.show_list(
                        ui,
                        ctx,
                        type_registry,
                        "Elements",
                        id.with("elements"),
                        element,
                        info,
                        ElementLabel::default(),
                        trail,
                    );
                }
                _ => {
                    Widgets::inspector_prop_value(ui, "Value ", |ui, _| {
                        ui.label(element.type_name_short());
                    });
                }
            },
        );
        (response, shown)
    }
}

/// Draws a dropdown of `info`'s variants, in declaration order, showing the
/// one `instance` holds, and sets `instance` to the variant picked. `label`
/// names the dropdown for accessibility. Reports a pick of another variant
/// as a discrete change.
fn unit_enum_dropdown(
    ui: &mut Ui,
    id: Id,
    label: &str,
    info: &EnumInfo,
    instance: &mut dyn Reflect,
) -> InspectorResponse {
    let Some(value) = instance.as_enum_mut() else {
        return InspectorResponse::NONE;
    };
    let current = value.variant_name();
    let current_label = info
        .variant(current)
        .map(variant_display_name)
        .unwrap_or_else(|| current.to_owned());
    let mut selected = current;
    let enabled = ui.is_enabled();
    let dropdown = egui::ComboBox::from_id_salt(id)
        .selected_text(current_label.as_str())
        .show_ui(ui, |ui| {
            for variant in &info.variants {
                ui.selectable_value(&mut selected, variant.name, variant_display_name(variant));
            }
        });
    dropdown.response.widget_info(|| {
        let mut widget = WidgetInfo::labeled(WidgetType::ComboBox, enabled, label);
        widget.current_text_value = Some(current_label.clone());
        widget
    });
    if selected != current && value.set_unit_variant(selected) {
        InspectorResponse::CHANGED
    } else {
        InspectorResponse::NONE
    }
}

/// The name the inspector shows for `variant`: its `name` attribute, or its
/// name in title case.
fn variant_display_name(variant: &EnumVariantInfo) -> String {
    match variant.attr("name") {
        Some(AttributeValue::String(name)) => name.to_owned(),
        _ => variant.name.from_case(Case::Pascal).to_case(Case::Title),
    }
}

/// The name the inspector shows for `instance`'s type: its `name` attribute,
/// or its short type name.
pub fn display_name(type_registry: &TypeRegistry, instance: &dyn Reflect) -> &'static str {
    type_registry
        .type_info_by_id(instance.uuid())
        .and_then(|info| {
            match info {
                TypeInfo::Struct(info) => {
                    if let Some(AttributeValue::String(str)) = info.attr("name") {
                        return Some(str);
                    }
                }
                TypeInfo::Enum(info) => {
                    if let Some(AttributeValue::String(str)) = info.attr("name") {
                        return Some(str);
                    }
                }
                _ => {}
            }
            None
        })
        .unwrap_or(instance.type_name_short())
}

/// The add-component menu category `instance`'s type names in its
/// `category` attribute, such as `"Post Processing"`, if any.
pub fn component_category(
    type_registry: &TypeRegistry,
    instance: &dyn Reflect,
) -> Option<&'static str> {
    let attr = match type_registry.type_info_by_id(instance.uuid())? {
        TypeInfo::Struct(info) => info.attr("category"),
        TypeInfo::Enum(info) => info.attr("category"),
        _ => None,
    };
    match attr {
        Some(AttributeValue::String(category)) => Some(category),
        _ => None,
    }
}

/// The name the inspector shows for the type registered as `type_uuid`, or
/// `None` when no such type is registered.
pub fn type_display_name(type_registry: &TypeRegistry, type_uuid: Uuid) -> Option<String> {
    type_registry
        .type_info_by_id(type_uuid)
        .map(|info| match info {
            TypeInfo::Struct(info) => info
                .attr("name")
                .and_then(|attr| match attr {
                    AttributeValue::String(name) => Some(name.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| short_type_name(info.type_name).to_string()),
            TypeInfo::Enum(info) => info
                .attr("name")
                .and_then(|attr| match attr {
                    AttributeValue::String(name) => Some(name.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| short_type_name(info.type_name).to_string()),
            TypeInfo::List(info) => short_type_name(info.type_name).to_string(),
            TypeInfo::Option(info) => short_type_name(info.type_name).to_string(),
            TypeInfo::Map(info) => short_type_name(info.type_name).to_string(),
            TypeInfo::None => type_uuid.to_string(),
        })
}

fn short_type_name(type_name: &str) -> &str {
    type_name.rsplit("::").next().unwrap_or(type_name)
}

fn field_display_name(field: &NamedField) -> String {
    if let Some(AttributeValue::String(name)) = field.attrs.get("name") {
        (*name).into()
    } else {
        field.name.from_case(Case::Snake).to_case(Case::Title)
    }
}

#[cfg(test)]
mod tests;
