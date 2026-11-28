//! Custom `iced::advanced::Widget` implementing true 2D dense Bento packing (`span × rows`),
//! live 60 FPS pointer drag-and-drop tile reordering with floating shadow layer & snap-slot
//! preview, and live bottom-right corner-drag 2D resizing.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, widget::Operation};
use iced::border::Radius;
use iced::mouse;
use iced::{
    Background, Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size,
    Theme, Vector,
};

/// Drag movement threshold (in logical pixels) before a pointer press promotes into a live tile drag,
/// preserving normal clicks on child buttons, links, and toggles.
pub const DRAG_THRESHOLD_PX: f32 = 6.0;

/// Height of the top draggable header strip active even when `Customize` mode is off.
pub const HEADER_DRAG_STRIP_PX: f32 = 38.0;

/// Hit-box size of the bottom-right resize corner when `Customize` mode is active.
pub const RESIZE_CORNER_EDIT_PX: f32 = 24.0;

/// Subtle hit-box size of the bottom-right resize corner on hover outside `Customize` mode.
pub const RESIZE_CORNER_HOVER_PX: f32 = 18.0;

/// Specification for a single Bento tile's identity and 2D grid span (`span × rows`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BentoSpec {
    pub id: String,
    pub span: u8,
    pub rows: u8,
}

impl BentoSpec {
    pub fn new(id: impl Into<String>, span: u8, rows: u8) -> Self {
        Self {
            id: id.into(),
            span: span.max(1),
            rows: rows.clamp(1, 3),
        }
    }
}

/// Computed 2D grid placement and pixel bounds (relative to `BentoGrid` top-left origin).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedTile {
    pub index: usize,
    pub col: usize,
    pub row: usize,
    pub span: usize,
    pub rows: usize,
    pub rect: Rectangle,
}

/// Output of the 2D dense Bento auto-placement algorithm.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedBento {
    pub placements: Vec<PlacedTile>,
    pub total_rows: usize,
    pub cell_w: f32,
    pub total_height: f32,
}

/// Packs variable-sized `(span × rows)` Bento tiles densely into a `cols`-wide 2D grid,
/// scanning row-by-row and left-to-right for the first conflict-free rectangle.
pub fn pack_bento_2d(
    specs: &[BentoSpec],
    cols: usize,
    avail_width: f32,
    row_height: f32,
    gap: f32,
) -> PackedBento {
    let cols = cols.max(1);
    let gap = gap.max(0.0);
    let row_height = row_height.max(40.0);
    let cell_w = ((avail_width - gap * (cols.saturating_sub(1) as f32)) / cols as f32).max(32.0);

    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut placements = Vec::with_capacity(specs.len());
    let mut max_row_end = 0usize;

    for (index, spec) in specs.iter().enumerate() {
        let w = (spec.span as usize).clamp(1, cols);
        let h = (spec.rows as usize).clamp(1, 3);

        let mut r = 0usize;
        let (placed_col, placed_row) = loop {
            while occupied.len() < r + h {
                occupied.push(vec![false; cols]);
            }

            let mut found_col = None;
            for c in 0..=(cols - w) {
                let mut fits = true;
                'check: for rr in r..(r + h) {
                    for cc in c..(c + w) {
                        if occupied[rr][cc] {
                            fits = false;
                            break 'check;
                        }
                    }
                }
                if fits {
                    found_col = Some(c);
                    break;
                }
            }

            if let Some(c) = found_col {
                for rr in r..(r + h) {
                    for cc in c..(c + w) {
                        occupied[rr][cc] = true;
                    }
                }
                break (c, r);
            }
            r += 1;
        };

        max_row_end = max_row_end.max(placed_row + h);

        let x = placed_col as f32 * (cell_w + gap);
        let y = placed_row as f32 * (row_height + gap);
        let width = w as f32 * cell_w + (w.saturating_sub(1) as f32) * gap;
        let height = h as f32 * row_height + (h.saturating_sub(1) as f32) * gap;

        placements.push(PlacedTile {
            index,
            col: placed_col,
            row: placed_row,
            span: w,
            rows: h,
            rect: Rectangle {
                x,
                y,
                width,
                height,
            },
        });
    }

    let total_height = if max_row_end == 0 {
        0.0
    } else {
        max_row_end as f32 * row_height + (max_row_end.saturating_sub(1) as f32) * gap
    };

    PackedBento {
        placements,
        total_rows: max_row_end,
        cell_w,
        total_height,
    }
}

/// Computes target `(span, rows)` from the cursor position relative to a tile's top-left anchor.
pub fn compute_resized_dims(
    tile_top_left: Point,
    cursor_pos: Point,
    cell_w: f32,
    row_height: f32,
    gap: f32,
    cols: usize,
) -> (u8, u8) {
    let cols_max = cols.clamp(1, 4) as f32;
    let raw_w = ((cursor_pos.x - tile_top_left.x + gap * 0.5) / (cell_w + gap)).round();
    let raw_h = ((cursor_pos.y - tile_top_left.y + gap * 0.5) / (row_height + gap)).round();
    let new_w = raw_w.clamp(1.0, cols_max) as u8;
    let new_h = raw_h.clamp(1.0, 3.0) as u8;
    (new_w, new_h)
}

/// Returns true if `pos` falls within the bottom-right corner resize handle of `rect`.
pub fn is_in_resize_corner(rect: Rectangle, pos: Point, handle_size: f32) -> bool {
    rect.contains(pos)
        && pos.x >= rect.x + rect.width - handle_size
        && pos.y >= rect.y + rect.height - handle_size
}

/// Returns true if `pos` falls within the top draggable header strip of `rect`.
pub fn is_in_header_strip(rect: Rectangle, pos: Point) -> bool {
    rect.contains(pos) && pos.y <= rect.y + HEADER_DRAG_STRIP_PX
}

/// Evaluates candidate splice positions `0..specs.len()` and returns a new target index only when
/// splicing the dragged tile there strictly moves its natural 2D slot closer to `cursor_local`
/// beyond a hysteresis deadband (eliminating variable-span ping-pong oscillation).
pub fn best_reorder_target(
    specs: &[BentoSpec],
    current_idx: usize,
    cursor_local: Point,
    cols: usize,
    avail_width: f32,
    row_height: f32,
    gap: f32,
) -> Option<usize> {
    if current_idx >= specs.len() || specs.len() < 2 {
        return None;
    }

    let base = pack_bento_2d(specs, cols, avail_width, row_height, gap);
    let cur_rect = base.placements[current_idx].rect;
    if cur_rect.contains(cursor_local) {
        return None;
    }

    let dist_sq = |a: Point, b: Point| {
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        dx * dx + dy * dy
    };

    let cur_d2 = dist_sq(cursor_local, cur_rect.center());
    let deadband_px = base.cell_w.min(row_height) * 0.20;

    let mut best_idx = current_idx;
    let mut best_d2 = cur_d2;
    let mut candidate_specs = Vec::with_capacity(specs.len());

    for cand_idx in 0..specs.len() {
        if cand_idx == current_idx {
            continue;
        }
        candidate_specs.clear();
        candidate_specs.extend_from_slice(specs);
        let item = candidate_specs.remove(current_idx);
        candidate_specs.insert(cand_idx, item);

        let sim = pack_bento_2d(&candidate_specs, cols, avail_width, row_height, gap);
        let sim_center = sim.placements[cand_idx].rect.center();
        let d2 = dist_sq(cursor_local, sim_center);
        if d2 < best_d2 {
            best_d2 = d2;
            best_idx = cand_idx;
        }
    }

    if best_idx != current_idx && best_d2.sqrt() + deadband_px < cur_d2.sqrt() {
        Some(best_idx)
    } else {
        None
    }
}

#[derive(Debug, Clone, Default)]
enum ActiveDrag {
    #[default]
    Idle,
    PendingMove {
        tile_id: String,
        press_pos: Point,
        grab_offset: Vector,
    },
    Moving {
        tile_id: String,
        grab_offset: Vector,
        cursor_pos: Point,
    },
    Resizing {
        tile_id: String,
        last_span: u8,
        last_rows: u8,
    },
}

#[derive(Debug, Default)]
pub struct BentoDragState {
    interaction: ActiveDrag,
    child_ids: Vec<String>,
    last_cursor_pos: Option<Point>,
}

fn cursor_any_position(cursor: mouse::Cursor) -> Option<Point> {
    match cursor {
        mouse::Cursor::Available(p) | mouse::Cursor::Levitating(p) => Some(p),
        mouse::Cursor::Unavailable => None,
    }
}

fn press_cursor_position(cursor: mouse::Cursor, fallback: Option<Point>) -> Option<Point> {
    match cursor {
        mouse::Cursor::Available(p) => Some(p),
        mouse::Cursor::Levitating(_) => None,
        mouse::Cursor::Unavailable => fallback,
    }
}

/// Responsive 2D Bento Grid container supporting live pointer drag-and-drop and corner resize.
pub struct BentoGrid<'a, Message> {
    children: Vec<Element<'a, Message>>,
    specs: Vec<BentoSpec>,
    cols: usize,
    row_height: f32,
    gap: f32,
    editing: bool,
    accent: Color,
    is_light: bool,
    tile_radius: f32,
    on_reorder: Box<dyn Fn(String, usize) -> Message + 'a>,
    on_resize: Box<dyn Fn(String, u8, u8) -> Message + 'a>,
}

impl<'a, Message> BentoGrid<'a, Message> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        children: Vec<Element<'a, Message>>,
        specs: Vec<BentoSpec>,
        cols: usize,
        row_height: f32,
        gap: f32,
        editing: bool,
        accent: Color,
        is_light: bool,
        tile_radius: f32,
        on_reorder: impl Fn(String, usize) -> Message + 'a,
        on_resize: impl Fn(String, u8, u8) -> Message + 'a,
    ) -> Self {
        debug_assert_eq!(children.len(), specs.len());
        Self {
            children,
            specs,
            cols: cols.max(1),
            row_height: row_height.max(80.0),
            gap: gap.max(0.0),
            editing,
            accent,
            is_light,
            tile_radius,
            on_reorder: Box::new(on_reorder),
            on_resize: Box::new(on_resize),
        }
    }

    fn resize_handle_size(&self) -> f32 {
        if self.editing {
            RESIZE_CORNER_EDIT_PX
        } else {
            RESIZE_CORNER_HOVER_PX
        }
    }

    fn draw_resize_grip(
        &self,
        renderer: &mut Renderer,
        tile_bounds: Rectangle,
        emphasized: bool,
    ) {
        use iced::advanced::Renderer as _;

        let inset = if self.tile_radius > 4.0 { 7.5 } else { 5.0 };
        let base_col = if emphasized {
            Color {
                a: if self.is_light { 0.72 } else { 0.78 },
                ..self.accent
            }
        } else if self.is_light {
            Color::from_rgba(0.12, 0.12, 0.10, 0.30)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.28)
        };

        // Crisp subtle 3-pip architectural corner grip (`◢`) at bottom-right
        let pip = 2.2_f32;
        let step = 4.2_f32;
        let rx = tile_bounds.x + tile_bounds.width - inset - pip;
        let ry = tile_bounds.y + tile_bounds.height - inset - pip;

        for &(gx, gy) in &[
            (0.0_f32, 0.0_f32),
            (-step, 0.0),
            (0.0, -step),
            (-step * 2.0, 0.0),
            (-step, -step),
            (0.0, -step * 2.0),
        ] {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: rx + gx,
                        y: ry + gy,
                        width: pip,
                        height: pip,
                    },
                    border: Border {
                        radius: Radius::from(1.0),
                        ..Default::default()
                    },
                    shadow: Shadow::default(),
                    snap: true,
                },
                Background::Color(base_col),
            );
        }
    }
}

impl<'a, Message> Widget<Message, Theme, Renderer> for BentoGrid<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<BentoDragState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(BentoDragState {
            interaction: ActiveDrag::Idle,
            child_ids: self.specs.iter().map(|s| s.id.clone()).collect(),
            last_cursor_pos: None,
        })
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<BentoDragState>();
        let mut old_entries: Vec<(String, Tree)> = state
            .child_ids
            .drain(..)
            .zip(tree.children.drain(..))
            .collect();

        let mut next_children = Vec::with_capacity(self.children.len());
        for (spec, child) in self.specs.iter().zip(self.children.iter()) {
            if let Some(pos) = old_entries.iter().position(|(id, _)| id == &spec.id) {
                let (_, mut existing_tree) = old_entries.swap_remove(pos);
                existing_tree.diff(child.as_widget());
                next_children.push(existing_tree);
            } else {
                next_children.push(Tree::new(child.as_widget()));
            }
        }

        tree.children = next_children;
        state.child_ids = self.specs.iter().map(|s| s.id.clone()).collect();
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max_width = limits.max().width;
        let packed = pack_bento_2d(
            &self.specs,
            self.cols,
            max_width,
            self.row_height,
            self.gap,
        );

        let mut nodes = Vec::with_capacity(self.children.len());
        for ((child, child_tree), placed) in self
            .children
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(packed.placements.iter())
        {
            let tile_size = Size::new(placed.rect.width, placed.rect.height);
            let child_limits = layout::Limits::new(tile_size, tile_size);
            let node = child
                .as_widget_mut()
                .layout(child_tree, renderer, &child_limits)
                .move_to(Point::new(placed.rect.x, placed.rect.y));
            nodes.push(node);
        }

        layout::Node::with_children(Size::new(max_width, packed.total_height), nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .zip(tree.children.iter_mut())
                .zip(layout.children())
                .for_each(|((child, state), child_layout)| {
                    child
                        .as_widget_mut()
                        .operate(state, child_layout, renderer, operation);
                });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<BentoDragState>();
        let grid_bounds = layout.bounds();
        let handle_sz = self.resize_handle_size();

        if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
            state.last_cursor_pos = Some(cursor_any_position(cursor).unwrap_or(*position));
        }
        let effective_pos = cursor_any_position(cursor).or(state.last_cursor_pos);

        // 1. Handle active Moving or Resizing interactions BEFORE forwarding to children
        match &mut state.interaction {
            ActiveDrag::Moving {
                tile_id,
                cursor_pos,
                ..
            } => {
                match event {
                    Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                        if let Some(pos) = effective_pos {
                            *cursor_pos = pos;
                            if let Some(cur_idx) =
                                self.specs.iter().position(|s| s.id == *tile_id)
                            {
                                let local = Point::new(
                                    pos.x - grid_bounds.x,
                                    pos.y - grid_bounds.y,
                                );
                                if let Some(target_idx) = best_reorder_target(
                                    &self.specs,
                                    cur_idx,
                                    local,
                                    self.cols,
                                    grid_bounds.width,
                                    self.row_height,
                                    self.gap,
                                ) {
                                    shell.publish((self.on_reorder)(
                                        tile_id.clone(),
                                        target_idx,
                                    ));
                                }
                            }
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        return;
                    }
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                        state.interaction = ActiveDrag::Idle;
                        if matches!(cursor, mouse::Cursor::Unavailable) {
                            state.last_cursor_pos = None;
                        }
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                    _ => return,
                }
            }
            ActiveDrag::Resizing {
                tile_id,
                last_span,
                last_rows,
            } => {
                match event {
                    Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                        if let Some(pos) = effective_pos {
                            if let Some(idx) = self.specs.iter().position(|s| s.id == *tile_id) {
                                let tile_bounds = layout.child(idx).bounds();
                                let cell_w = ((grid_bounds.width
                                    - self.gap * (self.cols.saturating_sub(1) as f32))
                                    / self.cols as f32)
                                    .max(32.0);
                                let (nw, nh) = compute_resized_dims(
                                    Point::new(tile_bounds.x, tile_bounds.y),
                                    pos,
                                    cell_w,
                                    self.row_height,
                                    self.gap,
                                    self.cols,
                                );
                                if nw != *last_span || nh != *last_rows {
                                    *last_span = nw;
                                    *last_rows = nh;
                                    shell.publish((self.on_resize)(tile_id.clone(), nw, nh));
                                }
                            }
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        return;
                    }
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                        state.interaction = ActiveDrag::Idle;
                        if matches!(cursor, mouse::Cursor::Unavailable) {
                            state.last_cursor_pos = None;
                        }
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                    _ => return,
                }
            }
            ActiveDrag::PendingMove {
                tile_id,
                press_pos,
                grab_offset,
            } => {
                if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event
                    && let Some(pos) = effective_pos
                {
                    let dx = pos.x - press_pos.x;
                    let dy = pos.y - press_pos.y;
                    if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD_PX {
                        let moved_id = tile_id.clone();
                        let offset = *grab_offset;
                        state.interaction = ActiveDrag::Moving {
                            tile_id: moved_id.clone(),
                            grab_offset: offset,
                            cursor_pos: pos,
                        };
                        // Cancel any child hover/press cleanly with an Unavailable cursor release
                        let cancel_ev =
                            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
                        for ((child, child_tree), child_layout) in self
                            .children
                            .iter_mut()
                            .zip(tree.children.iter_mut())
                            .zip(layout.children())
                        {
                            child.as_widget_mut().update(
                                child_tree,
                                &cancel_ev,
                                child_layout,
                                mouse::Cursor::Unavailable,
                                renderer,
                                clipboard,
                                shell,
                                viewport,
                            );
                        }
                        if let Some(cur_idx) =
                            self.specs.iter().position(|s| s.id == moved_id)
                        {
                            let local =
                                Point::new(pos.x - grid_bounds.x, pos.y - grid_bounds.y);
                            if let Some(target_idx) = best_reorder_target(
                                &self.specs,
                                cur_idx,
                                local,
                                self.cols,
                                grid_bounds.width,
                                self.row_height,
                                self.gap,
                            ) {
                                shell.publish((self.on_reorder)(moved_id, target_idx));
                            }
                        }
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                } else if let Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) =
                    event
                {
                    state.interaction = ActiveDrag::Idle;
                    if matches!(cursor, mouse::Cursor::Unavailable) {
                        state.last_cursor_pos = None;
                    }
                }
            }
            ActiveDrag::Idle => {}
        }

        // 2. Intercept left-press on bottom-right resize corner before children
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(pos) = press_cursor_position(cursor, state.last_cursor_pos)
        {
            for (idx, child_layout) in layout.children().enumerate() {
                if is_in_resize_corner(child_layout.bounds(), pos, handle_sz) {
                    let spec = &self.specs[idx];
                    state.interaction = ActiveDrag::Resizing {
                        tile_id: spec.id.clone(),
                        last_span: spec.span,
                        last_rows: spec.rows,
                    };
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
        }

        // 3. Forward normal events to children
        for ((child, child_tree), child_layout) in self
            .children
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                child_tree,
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
            if shell.is_event_captured() {
                return;
            }
        }

        // 4. If no child button/slider captured the press, check if a tile drag may start
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(pos) = press_cursor_position(cursor, state.last_cursor_pos)
        {
            for (idx, child_layout) in layout.children().enumerate() {
                let b = child_layout.bounds();
                if b.contains(pos) && (self.editing || is_in_header_strip(b, pos)) {
                    state.interaction = ActiveDrag::PendingMove {
                        tile_id: self.specs[idx].id.clone(),
                        press_pos: pos,
                        grab_offset: Vector::new(pos.x - b.x, pos.y - b.y),
                    };
                    break;
                }
            }
        } else if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) && matches!(cursor, mouse::Cursor::Unavailable)
        {
            state.last_cursor_pos = None;
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<BentoDragState>();

        match state.interaction {
            ActiveDrag::Moving { .. } => return mouse::Interaction::Grabbing,
            ActiveDrag::Resizing { .. } => return mouse::Interaction::ResizingDiagonallyDown,
            _ => {}
        }

        let handle_sz = self.resize_handle_size();
        if let Some(pos) = cursor.position() {
            for child_layout in layout.children() {
                if is_in_resize_corner(child_layout.bounds(), pos, handle_sz) {
                    return mouse::Interaction::ResizingDiagonallyDown;
                }
            }
        }

        for ((child, child_tree), child_layout) in self
            .children
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
        {
            let inter = child.as_widget().mouse_interaction(
                child_tree,
                child_layout,
                cursor,
                viewport,
                renderer,
            );
            if inter != mouse::Interaction::None {
                return inter;
            }
        }

        if let Some(pos) = cursor.position() {
            for child_layout in layout.children() {
                let b = child_layout.bounds();
                if b.contains(pos) && (self.editing || is_in_header_strip(b, pos)) {
                    return mouse::Interaction::Grab;
                }
            }
        }

        mouse::Interaction::None
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let state = tree.state.downcast_ref::<BentoDragState>();
        let dragged_info: Option<(&str, Vector, Point)> = match &state.interaction {
            ActiveDrag::Moving {
                tile_id,
                grab_offset,
                cursor_pos,
            } => Some((tile_id.as_str(), *grab_offset, *cursor_pos)),
            _ => None,
        };
        let resizing_id: Option<&str> = match &state.interaction {
            ActiveDrag::Resizing { tile_id, .. } => Some(tile_id.as_str()),
            _ => None,
        };

        let hover_pos = cursor.position();
        let handle_sz = self.resize_handle_size();
        let mut floating_idx: Option<(usize, Vector)> = None;

        for (idx, ((child, child_tree), child_layout)) in self
            .children
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
            .enumerate()
        {
            let b = child_layout.bounds();
            let spec_id = self.specs[idx].id.as_str();

            if let Some((drag_id, grab_offset, cur_pos)) = dragged_info
                && drag_id == spec_id
            {
                // Draw crisp snap-slot placeholder in the dragged tile's natural grid slot
                renderer.fill_quad(
                    Quad {
                        bounds: b,
                        border: Border {
                            color: Color {
                                a: 0.62,
                                ..self.accent
                            },
                            width: 1.5,
                            radius: Radius::from(self.tile_radius),
                        },
                        shadow: Shadow::default(),
                        snap: true,
                    },
                    Background::Color(Color {
                        a: if self.is_light { 0.08 } else { 0.10 },
                        ..self.accent
                    }),
                );

                let target_top_left =
                    Point::new(cur_pos.x - grab_offset.x, cur_pos.y - grab_offset.y);
                let delta = Vector::new(target_top_left.x - b.x, target_top_left.y - b.y);
                floating_idx = Some((idx, delta));
                continue;
            }

            if b.intersects(viewport) {
                let child_cursor = if dragged_info.is_some() {
                    mouse::Cursor::Unavailable
                } else {
                    cursor
                };
                child.as_widget().draw(
                    child_tree,
                    renderer,
                    theme,
                    style,
                    child_layout,
                    child_cursor,
                    viewport,
                );

                let is_hovered =
                    dragged_info.is_none() && hover_pos.is_some_and(|p| b.contains(p));
                let is_resizing_this = resizing_id == Some(spec_id);

                // Whisper-quiet hover border lift (0.065 -> ~0.15) or active resize frame
                if is_resizing_this {
                    renderer.fill_quad(
                        Quad {
                            bounds: b,
                            border: Border {
                                color: Color {
                                    a: 0.78,
                                    ..self.accent
                                },
                                width: 1.5,
                                radius: Radius::from(self.tile_radius),
                            },
                            shadow: Shadow::default(),
                            snap: true,
                        },
                        Background::Color(Color::TRANSPARENT),
                    );
                } else if is_hovered && !self.editing {
                    let lift_col = if self.is_light {
                        Color::from_rgba(0.10, 0.10, 0.08, 0.11)
                    } else {
                        Color::from_rgba(1.0, 1.0, 1.0, 0.095)
                    };
                    renderer.fill_quad(
                        Quad {
                            bounds: b,
                            border: Border {
                                color: lift_col,
                                width: 1.0,
                                radius: Radius::from(self.tile_radius),
                            },
                            shadow: Shadow::default(),
                            snap: true,
                        },
                        Background::Color(Color::TRANSPARENT),
                    );
                }

                // Subtle bottom-right resize grip when Customize is active or tile is hovered
                if self.editing || is_hovered || is_resizing_this {
                    let corner_hot = is_resizing_this
                        || hover_pos.is_some_and(|p| is_in_resize_corner(b, p, handle_sz));
                    self.draw_resize_grip(renderer, b, corner_hot || self.editing);
                }
            }
        }

        // Draw floating dragged tile in a top-most GPU layer with deep soft elevation shadow
        if let Some((idx, delta)) = floating_idx {
            let child = &self.children[idx];
            let child_tree = &tree.children[idx];
            let child_layout = layout.child(idx);
            let b = child_layout.bounds();
            let floated_bounds = Rectangle {
                x: b.x + delta.x,
                y: b.y + delta.y,
                width: b.width,
                height: b.height,
            };

            let layer_bounds = viewport.union(&floated_bounds);
            renderer.with_layer(layer_bounds, |r| {
                r.fill_quad(
                    Quad {
                        bounds: floated_bounds,
                        border: Border {
                            color: Color {
                                a: 0.72,
                                ..self.accent
                            },
                            width: 1.5,
                            radius: Radius::from(self.tile_radius),
                        },
                        shadow: Shadow {
                            color: Color::from_rgba(0.0, 0.0, 0.0, 0.58),
                            offset: Vector::new(0.0, 14.0),
                            blur_radius: 34.0,
                        },
                        snap: false,
                    },
                    Background::Color(Color::TRANSPARENT),
                );

                let local_vp = Rectangle {
                    x: layer_bounds.x - delta.x,
                    y: layer_bounds.y - delta.y,
                    width: layer_bounds.width,
                    height: layer_bounds.height,
                };
                r.with_translation(delta, |r| {
                    child.as_widget().draw(
                        child_tree,
                        r,
                        theme,
                        style,
                        child_layout,
                        mouse::Cursor::Unavailable,
                        &local_vp,
                    );
                    self.draw_resize_grip(r, b, true);
                });
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message> From<BentoGrid<'a, Message>> for Element<'a, Message>
where
    Message: 'a,
{
    fn from(grid: BentoGrid<'a, Message>) -> Self {
        Element::new(grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_2d_packing_places_four_1x1_beside_2x2_hero() {
        let specs = vec![
            BentoSpec::new("lighting", 2, 2),
            BentoSpec::new("cpu-temp", 1, 1),
            BentoSpec::new("gpu-temp", 1, 1),
            BentoSpec::new("cpu-load", 1, 1),
            BentoSpec::new("gpu-load", 1, 1),
            BentoSpec::new("profiles", 2, 1),
            BentoSpec::new("power", 2, 1),
            BentoSpec::new("fans", 1, 1),
            BentoSpec::new("ram", 1, 1),
            BentoSpec::new("drive", 1, 1),
            BentoSpec::new("net", 1, 1),
        ];

        let packed = pack_bento_2d(&specs, 4, 1382.0, 180.0, 14.0);
        assert_eq!(packed.total_rows, 4);

        // Lighting (2x2) at (0..2, 0..2)
        assert_eq!((packed.placements[0].col, packed.placements[0].row), (0, 0));
        assert_eq!(
            (packed.placements[0].span, packed.placements[0].rows),
            (2, 2)
        );
        assert!((packed.placements[0].rect.height - (180.0 * 2.0 + 14.0)).abs() < 0.01);

        // Four 1x1s pack into cols 2..4, rows 0..2 right beside Lighting!
        assert_eq!((packed.placements[1].col, packed.placements[1].row), (2, 0));
        assert_eq!((packed.placements[2].col, packed.placements[2].row), (3, 0));
        assert_eq!((packed.placements[3].col, packed.placements[3].row), (2, 1));
        assert_eq!((packed.placements[4].col, packed.placements[4].row), (3, 1));

        // Profiles (2x1) + Power (2x1) on row 2
        assert_eq!((packed.placements[5].col, packed.placements[5].row), (0, 2));
        assert_eq!((packed.placements[6].col, packed.placements[6].row), (2, 2));

        // Four 1x1 bottom cards on row 3
        for (i, expected_col) in (7..11).zip(0..4) {
            assert_eq!(
                (packed.placements[i].col, packed.placements[i].row),
                (expected_col, 3)
            );
        }
    }

    #[test]
    fn resize_and_corner_hit_test_math_clamps_cleanly() {
        let origin = Point::new(100.0, 200.0);
        let cell_w = 320.0;
        let row_h = 180.0;
        let gap = 14.0;

        // Dragging inside 1x1 stays (1, 1)
        assert_eq!(
            compute_resized_dims(
                origin,
                Point::new(100.0 + 290.0, 200.0 + 150.0),
                cell_w,
                row_h,
                gap,
                4
            ),
            (1, 1)
        );
        // Dragging across into 2nd col & 2nd row snaps to (2, 2)
        assert_eq!(
            compute_resized_dims(
                origin,
                Point::new(100.0 + 610.0, 200.0 + 340.0),
                cell_w,
                row_h,
                gap,
                4
            ),
            (2, 2)
        );
        // Clamps max span to cols and max rows to 3
        assert_eq!(
            compute_resized_dims(
                origin,
                Point::new(2500.0, 2000.0),
                cell_w,
                row_h,
                gap,
                4
            ),
            (4, 3)
        );

        let rect = Rectangle {
            x: 100.0,
            y: 200.0,
            width: 320.0,
            height: 180.0,
        };
        assert!(is_in_resize_corner(
            rect,
            Point::new(412.0, 372.0),
            RESIZE_CORNER_EDIT_PX
        ));
        assert!(!is_in_resize_corner(
            rect,
            Point::new(250.0, 250.0),
            RESIZE_CORNER_EDIT_PX
        ));
        assert!(is_in_header_strip(rect, Point::new(220.0, 224.0)));
        assert!(!is_in_header_strip(rect, Point::new(220.0, 260.0)));
    }

    #[test]
    fn reorder_solver_moves_toward_cursor_without_ping_pong() {
        let mut specs = vec![
            BentoSpec::new("lighting", 2, 2),
            BentoSpec::new("cpu-temp", 1, 1),
            BentoSpec::new("gpu-temp", 1, 1),
            BentoSpec::new("cpu-load", 1, 1),
            BentoSpec::new("gpu-load", 1, 1),
        ];
        let packed = pack_bento_2d(&specs, 4, 1382.0, 180.0, 14.0);
        let gpu_center = packed.placements[2].rect.center();

        // Dragging cpu-temp (idx 1) onto gpu-temp's center (idx 2) selects idx 2
        let target = best_reorder_target(&specs, 1, gpu_center, 4, 1382.0, 180.0, 14.0);
        assert_eq!(target, Some(2));

        // Apply splice and verify a second query at the SAME cursor position does NOT ping-pong!
        let moved = specs.remove(1);
        specs.insert(2, moved);
        let second = best_reorder_target(&specs, 2, gpu_center, 4, 1382.0, 180.0, 14.0);
        assert_eq!(second, None);
    }

    #[test]
    fn hidpi_scaled_logical_grid_pack_drag_and_resize_match_exact_geometry() {
        // 2560x1440 @ 1.25x scale_factor -> 2048x1152 logical canvas, 28px gutters -> 1992px grid
        let specs = vec![
            BentoSpec::new("lighting", 2, 2),
            BentoSpec::new("cpu-temp", 1, 1),
            BentoSpec::new("gpu-temp", 1, 1),
            BentoSpec::new("cpu-load", 1, 1),
            BentoSpec::new("gpu-load", 1, 1),
        ];
        let packed = pack_bento_2d(&specs, 4, 1992.0, 238.5, 14.0);
        assert!((packed.cell_w - 487.5).abs() < 0.01);
        assert_eq!(packed.total_rows, 2);

        let origin = Point::new(packed.placements[1].rect.x, packed.placements[1].rect.y);
        let resized = compute_resized_dims(
            origin,
            Point::new(origin.x + packed.cell_w * 2.0 + 14.0, origin.y + 238.5),
            packed.cell_w,
            238.5,
            14.0,
            4,
        );
        assert_eq!(resized, (2, 1));

        let gpu_center = packed.placements[2].rect.center();
        assert_eq!(
            best_reorder_target(&specs, 1, gpu_center, 4, 1992.0, 238.5, 14.0),
            Some(2)
        );
    }
}
