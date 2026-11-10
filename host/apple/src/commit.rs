//! Commit receipts into the host mirror, motion engine and layout.
use super::*;

impl<D: DataSource> Host<D> {
    /// The receipts into the mirror, the engine and layout; not yet presented.
    pub(super) fn commit_tree(
        &mut self,
        receipts: &[Timed],
        error: Option<String>,
        mut batch: Batch,
    ) -> (Batch, Option<String>) {
        let _roles = self.runner.bind_roles(); // Its own, crossing (LLP 1121 stage 5).
        self.retained_begin();
        self.native_retire_removed_owner(&mut batch);
        self.native_note_receipts(receipts);
        let bulk_handlers = (receipts
            .iter()
            .map(|t| t.receipt.created.len())
            .sum::<usize>()
            > 1)
        .then(|| self.runner.handlers());
        self.close_new_top_layer(receipts);
        for t in receipts {
            let r = &t.receipt;
            // Flights capture their leavers before an exit takes them
            // out of the presenter's maps (LLP 1013.000 D4.1).
            self.begin_flights(r, &mut batch);
            self.begin_exits(r, &mut batch);
            for key in &r.destroyed {
                if let Some(id) = self.keys.remove(key) {
                    self.retained_forget(id);
                    self.ranks.remove(&id);
                    self.paint.runs.remove(&id);
                    if let Some((owner, _)) = self.inline_runs.remove(&id) {
                        self.dirty_paragraphs.insert(owner);
                    } else if self.svg.destroyed(id) {
                        self.mirror.remove(&id);
                    } else if !self.native_selected_id(id) {
                        self.mirror.remove(&id);
                        self.end_flight_of(id);
                        if !self.exit_holds(id, &mut batch) {
                            batch.destroy(id);
                        }
                    }
                    self.transform_drags.remove(id);
                }
            }
            for key in &r.created {
                if let Some(node) = self.runner.kernel().node_by_key(*key) {
                    let id = node.id;
                    let handlers = bulk_handlers.as_ref().map_or_else(
                        || self.runner.handlers_of(id),
                        |all| all.get(&id).cloned().unwrap_or_default(),
                    );
                    self.create(id, &handlers, &mut batch);
                }
            }
            for key in &r.touched {
                if let Some(node) = self.runner.kernel().node_by_key(*key) {
                    let id = node.id;
                    self.update(id, &mut batch);
                }
            }
        }
        // Receipts share the final kernel tree. A parent touched by an early
        // timer can already name a child created by a later timer in this seek.
        // All surviving views must exist before any final child list is attached.
        let released = self.retained_release(&mut batch);
        for id in released {
            self.emit_children(id, &mut batch);
        }
        for t in receipts {
            for key in t.receipt.created.iter().chain(t.receipt.touched.iter()) {
                if let Some(node) = self.runner.kernel().node_by_key(*key) {
                    self.emit_children(node.id, &mut batch);
                }
            }
        }
        self.emit_paragraphs(&mut batch);
        let roots = self.runner.roots();
        if roots != self.roots {
            self.roots = roots.clone();
            batch.roots(&roots);
        }
        if !receipts.is_empty() {
            self.emit_title(&mut batch);
        }
        let mut height_target_error = None;
        // Runner receipts retain due-time order. Unobserved motion starts at
        // that due time; a late receipt cannot rewind an already presented
        // frame/hold. Match Web's floor at the engine's current presentation
        // time, not this batch's final time. No timer work enters pointer moves.
        for t in receipts {
            let seek = self.engine.advance(self.engine_time(t.at_ms));
            debug_assert!(seek.is_ok(), "the clock never runs backwards here");
            let mut sync = self.runner.kernel().motion_sync(&t.receipt);
            self.spare_exits(&mut sync);
            let applied = sync.apply(&mut self.engine);
            debug_assert!(applied.is_ok(), "kernel rows are always valid engine input");
            let runner = &self.runner;
            svg_lower::eligibility(
                runner.kernel(),
                &mut self.engine,
                &sync,
                self.svg.box_motion,
                &|v| !runner.handlers_of(v).is_empty(),
            );
            svg_lower::hand_over_paths(runner.kernel(), &mut self.engine, self.svg.box_motion);
            // A reveal changes a held play without changing its style row.
            for (key, _) in &sync.animations {
                if let Some(view) = self.keys.get(&node_key(*key)).copied() {
                    self.svg.element(runner.kernel(), view);
                }
            }
            for view in svg_lower::glass_sampling(self.runner.kernel(), &mut self.engine) {
                self.svg.element(self.runner.kernel(), view);
            }
            self.play_exits(&mut batch);
            self.start_flights(&t.receipt);
            self.seed_layout(&t.receipt, &mut batch);
            self.sync_paint(&t.receipt, &mut batch);
            self.reconcile_height_handles(&mut batch, true);
            let synced = self.sync_height_owner();
            debug_assert!(synced.is_ok(), "validated height sync");
            if let Err(error) = self.sync_height_transitions() {
                height_target_error.get_or_insert(error);
            }
            // Latest target/declaration must reach the held slot before an
            // invalidated header cancels it (negative delays sample at once).
            self.cancel_invalid_height_drag();
            self.reconcile_transform_drags(&mut batch);
        }
        let seek = self.engine.advance(self.engine_time(self.now_ms));
        debug_assert!(seek.is_ok(), "the clock never runs backwards here");
        // An advance whose commits changed no node (a frame task that only
        // moved state nothing shows; a `sent` reader answered at each MIDI
        // message's time, LLP 1125 D6, with nothing on screen reading it)
        // lays out only what such a commit can move (`inert_layout`): each
        // would cost a whole layout and paint order walk otherwise.
        let inert = receipts.iter().all(|t| unchanged(&t.receipt));
        let layout_error = if height_target_error.is_some() {
            self.layout_owed = true;
            height_target_error
        } else if inert {
            self.inert_layout(&mut batch, !receipts.is_empty()).err()
        } else {
            self.layout(&mut batch).err()
        };
        if layout_error.is_some() && !receipts.is_empty() {
            self.emit_ranks(&mut batch);
        }
        self.canvas_turn(&mut batch);
        for s in self.runner.take_surface_updates() {
            batch.surface(&s);
        }
        // The capabilities the actions called, after their commits, in order.
        // @ref LLP 1038 D7 — drain once, after all commits in this batch.
        if let Some(change) = self.runner.take_router_change() {
            batch.router(&change);
        }
        for c in self.runner.take_commands() {
            batch.command(&c.name, &c.args, c.source);
        }
        batch.timed(&mut self.runner, false);
        (batch, error.or(layout_error))
    }
}
