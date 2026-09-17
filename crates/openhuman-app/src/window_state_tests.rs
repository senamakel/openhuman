use super::*;

#[test]
fn window_state_toml_roundtrips() {
    // `save_main` serializes this record to `window_state.toml` and
    // `restore_main` parses it back on the next launch. The whole
    // save-on-quit → restore-on-launch feature (#4810) hinges on this
    // record surviving the round trip byte-for-byte, including a
    // negative x from a monitor left of the primary. Guards the
    // on-disk contract against an accidental serde/rename change.
    let state = WindowState {
        x: -1920,
        y: 100,
        width: 1280,
        height: 800,
    };
    let raw = toml::to_string_pretty(&state).expect("serialize");
    let parsed: WindowState = toml::from_str(&raw).expect("parse");
    assert_eq!(
        (parsed.x, parsed.y, parsed.width, parsed.height),
        (state.x, state.y, state.width, state.height)
    );
}

fn wa(x: i32, y: i32, width: u32, height: u32) -> WorkArea {
    wa_dpi(x, y, width, height, 1.0)
}

/// Same as [`wa`] but with an explicit DPI scale factor, for the
/// mixed-DPI cases in #5041.
fn wa_dpi(x: i32, y: i32, width: u32, height: u32, scale: f64) -> WorkArea {
    WorkArea {
        x,
        y,
        width,
        height,
        scale,
    }
}

/// The reporter's layout from #5041: a 2560x1600 @ 150 % primary at
/// the virtual-desktop origin, and a 1920x1080 @ 100 % secondary
/// placed to its **left** (so the secondary's origin is negative).
/// Work areas subtract a plausible taskbar.
fn reporter_layout() -> (WorkArea, WorkArea) {
    let primary = wa_dpi(0, 0, 2560, 1520, 1.5);
    let secondary = wa_dpi(-1920, 0, 1920, 1032, 1.0);
    (primary, secondary)
}

#[test]
fn clamp_leaves_in_bounds_geometry_alone() {
    // 1280×800 work area, 1000×800 window centered-ish: width fits,
    // height fits exactly — nothing should change.
    let work_area = wa(0, 0, 1280, 800);
    let (x, y, w, h) = clamp_to_work_area(100, 0, 1000, 800, work_area);
    assert_eq!((x, y, w, h), (100, 0, 1000, 800));
}

#[test]
fn clamp_shrinks_window_taller_than_work_area() {
    // Repro for #2282: default 1000×800 on a 1280×720 work area
    // (e.g. macOS 13" Air with menu bar+dock visible). Height
    // shrinks to the work area height so bottom nav stays visible.
    let work_area = wa(0, 0, 1280, 720);
    let (x, y, w, h) = clamp_to_work_area(0, 0, 1000, 800, work_area);
    assert_eq!(w, 1000);
    assert_eq!(h, 720);
    assert_eq!((x, y), (0, 0));
}

#[test]
fn clamp_shrinks_window_wider_than_work_area() {
    let work_area = wa(0, 0, 800, 600);
    let (_, _, w, h) = clamp_to_work_area(0, 0, 1600, 1200, work_area);
    assert_eq!(w, 800);
    assert_eq!(h, 600);
}

#[test]
fn clamp_respects_minimum_size_on_tiny_work_area() {
    // Pathological tiny work area: don't shrink below the usability
    // floor. (User can still scroll/resize; better than a 0×0 sliver.)
    let work_area = wa(0, 0, 200, 150);
    let (_, _, w, h) = clamp_to_work_area(0, 0, 1000, 800, work_area);
    assert_eq!(w, MIN_WINDOW_WIDTH);
    assert_eq!(h, MIN_WINDOW_HEIGHT);
}

#[test]
fn clamp_pushes_window_back_inside_when_off_right_or_bottom() {
    // Saved at (1200, 700) sized 1000×800 — bottom-right is far
    // outside the 1280×800 work area. Position should shift left
    // and up so the *whole frame* fits inside the work area.
    let work_area = wa(0, 0, 1280, 800);
    let (x, y, w, h) = clamp_to_work_area(1200, 700, 1000, 800, work_area);
    assert_eq!(w, 1000);
    assert_eq!(h, 800);
    assert_eq!(x, 1280 - 1000);
    assert_eq!(y, 0);
}

#[test]
fn clamp_handles_negative_position_on_offset_monitor() {
    // Secondary monitor positioned to the left of the primary —
    // origin is at (-1920, 0). A saved window slightly left of that
    // monitor's left edge should be pulled inward.
    let work_area = wa(-1920, 0, 1920, 1080);
    let (x, y, _, _) = clamp_to_work_area(-2000, 100, 1000, 800, work_area);
    assert_eq!(x, -1920);
    assert_eq!(y, 100);
}

#[test]
fn work_area_fill_uses_the_whole_work_area_on_a_normal_monitor() {
    let (x, y, w, h) = work_area_fill_geometry(wa(0, 60, 3600, 2190));
    assert_eq!((x, y), (0, 60));
    assert_eq!((w, h), (3600, 2190));
}

#[test]
fn work_area_fill_keeps_the_offset_of_a_secondary_monitor() {
    // A monitor to the left of the primary has a negative origin; filling
    // its work area must land there, not at (0, 0).
    let (x, y, w, h) = work_area_fill_geometry(wa(-1920, 0, 1920, 1080));
    assert_eq!((x, y), (-1920, 0));
    assert_eq!((w, h), (1920, 1080));
}

#[test]
fn work_area_fill_never_goes_below_the_minimum_window_size() {
    // The invariant this helper exists for: a work area smaller than
    // MIN_WINDOW_* must still produce at least the minimum, matching what
    // `restore_main` and `center_main` guarantee.
    let (_, _, w, h) = work_area_fill_geometry(wa(0, 0, 320, 200));
    assert_eq!((w, h), (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT));
}

#[test]
fn clamp_size_only_caps_to_work_area() {
    let work_area = wa(0, 0, 1024, 600);
    let (w, h) = clamp_size(1600, 1200, &work_area);
    assert_eq!((w, h), (1024, 600));
}

#[test]
fn clamp_size_below_minimum_floor_returns_minimum() {
    let work_area = wa(0, 0, 200, 150);
    let (w, h) = clamp_size(100, 50, &work_area);
    assert_eq!((w, h), (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT));
}

#[test]
fn pick_monitor_finds_overlapping_monitor() {
    let monitors = vec![wa(0, 0, 1920, 1080), wa(1920, 0, 1280, 800)];
    // Window sits on the secondary monitor (right of primary).
    let m = pick_monitor_for_window(2000, 100, 1000, 700, &monitors).unwrap();
    assert_eq!((m.x, m.width), (1920, 1280));
}

#[test]
fn pick_monitor_returns_none_when_window_off_every_screen() {
    // Saved on a now-disconnected display (large positive offset).
    let monitors = vec![wa(0, 0, 1920, 1080)];
    let m = pick_monitor_for_window(5000, 5000, 1000, 800, &monitors);
    assert!(
        m.is_none(),
        "off-screen window should not match any monitor"
    );
}

#[test]
fn pick_monitor_requires_minimum_overlap() {
    // Window only intersects the monitor by a 50px sliver — below
    // the 100px threshold, so we treat it as off-screen.
    let monitors = vec![wa(0, 0, 1920, 1080)];
    let m = pick_monitor_for_window(-950, 100, 1000, 700, &monitors);
    assert!(m.is_none(), "sub-threshold overlap should not match");
}

#[test]
fn pick_monitor_handles_empty_list() {
    let m = pick_monitor_for_window(0, 0, 1000, 800, &[]);
    assert!(m.is_none());
}

#[test]
fn pick_monitor_prefers_largest_overlap_for_straddling_window() {
    // Window at (1820, 0) with 1000×700 straddles two horizontally
    // adjacent monitors: 100×700 = 70 000 px² on the primary,
    // 900×700 = 630 000 px² on the secondary. Must pick the
    // secondary regardless of `available_monitors()` ordering.
    let primary = wa(0, 0, 1920, 1080);
    let secondary = wa(1920, 0, 1280, 800);
    // Primary first.
    let m = pick_monitor_for_window(1820, 0, 1000, 700, &[primary, secondary]).unwrap();
    assert_eq!((m.x, m.width), (1920, 1280));
    // Secondary first — same answer.
    let m = pick_monitor_for_window(1820, 0, 1000, 700, &[secondary, primary]).unwrap();
    assert_eq!((m.x, m.width), (1920, 1280));
}

// ── Mixed-DPI multi-monitor (#5041) ──────────────────────────────

#[test]
fn dpi_adjusted_size_grows_when_moving_to_a_higher_dpi_monitor() {
    // The #5041 arithmetic: a window fitted to the 150 % primary's
    // 2560 px work-area width while still sitting on the 100 %
    // secondary arrives at 3840 px — wider than the very monitor it
    // was just fitted to. This is the reported ~3875 px window.
    let (grown_w, grown_h) = dpi_adjusted_size(2560, 1520, 1.0, 1.5);
    assert_eq!(grown_w, 3840);
    assert_eq!(grown_h, 2280);
}

#[test]
fn dpi_adjusted_size_shrinks_when_moving_to_a_lower_dpi_monitor() {
    let (w, h) = dpi_adjusted_size(3840, 2280, 1.5, 1.0);
    assert_eq!((w, h), (2560, 1520));
}

#[test]
fn dpi_adjusted_size_is_identity_at_equal_scale() {
    assert_eq!(dpi_adjusted_size(1280, 900, 1.5, 1.5), (1280, 900));
    assert_eq!(dpi_adjusted_size(1280, 900, 1.0, 1.0), (1280, 900));
}

#[test]
fn dpi_adjusted_size_round_trips_without_drift() {
    // A→B→A must land back on the original size; truncating instead
    // of rounding would lose a pixel on every move.
    //
    // Covers the whole standard DPI ladder rather than just 1.0↔1.5:
    // a single pair does not establish the property (review, #5041).
    // Odd dimensions are deliberate — even ones round-trip trivially.
    const SIZES: [(u32, u32); 3] = [(1281, 901), (1920, 1080), (2560, 1600)];

    // Upscale first: exact. This is the common case — a window sized
    // on a low-DPI monitor moves to a high-DPI one and back.
    for (from, to) in [
        (1.0, 1.25),
        (1.0, 1.5),
        (1.0, 1.75),
        (1.0, 2.0),
        (1.25, 1.5),
        (1.5, 2.0),
    ] {
        for (w, h) in SIZES {
            let (up_w, up_h) = dpi_adjusted_size(w, h, from, to);
            let (back_w, back_h) = dpi_adjusted_size(up_w, up_h, to, from);
            assert_eq!(
                (back_w, back_h),
                (w, h),
                "upscale-first round trip {w}x{h} at {from}->{to}->{from} drifted to {back_w}x{back_h}"
            );
        }
    }

    // Downscale first: bounded, not exact. The intermediate size has
    // genuinely lost information (1281 at 2.0->1.25 is 801, and 801
    // back up is 1282), so the contract is that the error stays
    // within 1 px and never accumulates — which is precisely what
    // truncation fails to do.
    for (from, to) in [(2.0, 1.25), (1.75, 1.0), (2.0, 1.0), (1.5, 1.25)] {
        for (w, h) in SIZES {
            let (down_w, down_h) = dpi_adjusted_size(w, h, from, to);
            let (back_w, back_h) = dpi_adjusted_size(down_w, down_h, to, from);
            assert!(
                back_w.abs_diff(w) <= 1 && back_h.abs_diff(h) <= 1,
                "downscale-first round trip {w}x{h} at {from}->{to}->{from} drifted to {back_w}x{back_h}, beyond the 1px bound"
            );
        }
    }
}

#[test]
fn dpi_adjusted_size_rejects_unusable_scale_factors() {
    // A runtime reporting 0.0 / NaN / infinity must not zero out or
    // panic the window size — return the input untouched instead.
    for (from, to) in [
        (0.0, 1.5),
        (1.5, 0.0),
        (f64::NAN, 1.5),
        (1.5, f64::NAN),
        (f64::INFINITY, 1.5),
        (1.5, f64::INFINITY),
        (-1.0, 1.5),
    ] {
        assert_eq!(
            dpi_adjusted_size(1280, 900, from, to),
            (1280, 900),
            "scale pair ({from}, {to}) must be treated as unusable"
        );
    }
}

#[test]
fn target_prefers_the_monitor_the_window_is_already_on() {
    // Window created on the 100 % secondary (negative origin).
    // Selecting the primary here is what forces the cross-DPI move
    // that #5041 is about — we must stay put instead.
    let (primary, secondary) = reporter_layout();
    let target = target_work_area_for_new_window(
        -1800,
        100,
        1280,
        900,
        &[primary, secondary],
        Some(primary),
    )
    .expect("a monitor should be selected");
    assert_eq!(
        (target.x, target.width),
        (secondary.x, secondary.width),
        "window on the secondary must be sized against the secondary"
    );
    assert!(
        (target.scale - secondary.scale).abs() < f64::EPSILON,
        "and must carry the secondary's scale factor"
    );
}

#[test]
fn target_falls_back_to_primary_when_window_overlaps_nothing() {
    // Windows can place a not-yet-shown window at CW_USEDEFAULT,
    // off every work area.
    let (primary, secondary) = reporter_layout();
    let target = target_work_area_for_new_window(
        50_000,
        50_000,
        1280,
        900,
        &[primary, secondary],
        Some(primary),
    )
    .expect("should fall back to primary");
    assert_eq!((target.x, target.width), (primary.x, primary.width));
}

#[test]
fn target_falls_back_to_any_monitor_without_a_primary() {
    let (primary, secondary) = reporter_layout();
    let target =
        target_work_area_for_new_window(50_000, 50_000, 1280, 900, &[secondary, primary], None)
            .expect("should fall back to the first attached monitor");
    assert_eq!(target.x, secondary.x);
}

#[test]
fn target_returns_none_without_monitors() {
    assert!(target_work_area_for_new_window(0, 0, 1280, 900, &[], None).is_none());
}

#[test]
fn clamp_then_move_overflows_but_move_then_clamp_fits() {
    // End-to-end regression for #5041, expressed against the pure
    // helpers so it runs without a Tauri runtime.
    //
    // Setup: an oversized window (larger than either work area) is
    // created on the 100 % secondary and needs to end up on the
    // 150 % primary.
    let (primary, _secondary) = reporter_layout();
    let (created_w, created_h) = (3000, 1700);

    // OLD ordering — clamp to the destination, then move. The OS
    // rescales by 1.5 on arrival and the result overflows the very
    // monitor it was fitted to.
    let (clamped_w, clamped_h) = clamp_size(created_w, created_h, &primary);
    assert_eq!(
        (clamped_w, clamped_h),
        (2560, 1520),
        "clamp itself is correct"
    );
    let (arrived_w, arrived_h) = dpi_adjusted_size(clamped_w, clamped_h, 1.0, 1.5);
    assert!(
        arrived_w > primary.width && arrived_h > primary.height,
        "old ordering must reproduce the oversized window: {arrived_w}x{arrived_h} vs work area {}x{}",
        primary.width,
        primary.height
    );

    // NEW ordering — move first (absorbing the OS rescale), then
    // clamp against the destination. Result fits.
    let (moved_w, moved_h) = dpi_adjusted_size(created_w, created_h, 1.0, 1.5);
    let (final_w, final_h) = clamp_size(moved_w, moved_h, &primary);
    assert!(
        final_w <= primary.width && final_h <= primary.height,
        "new ordering must fit the work area: {final_w}x{final_h} vs {}x{}",
        primary.width,
        primary.height
    );
}

#[test]
fn reclamped_geometry_never_leaves_the_work_area_after_a_scale_change() {
    // What `reclamp_after_scale_change` delegates to: whatever size
    // the OS imposed on arrival, the window must end up wholly
    // inside the destination work area.
    let (primary, _) = reporter_layout();
    let (os_w, os_h) = dpi_adjusted_size(2560, 1520, 1.0, 1.5);
    let (x, y, w, h) = clamp_to_work_area(0, 0, os_w, os_h, primary);
    assert!(w <= primary.width && h <= primary.height);
    assert!(x >= primary.x && y >= primary.y);
    assert!(x.saturating_add(w as i32) <= primary.x.saturating_add(primary.width as i32));
    assert!(y.saturating_add(h as i32) <= primary.y.saturating_add(primary.height as i32));
}

#[test]
fn center_origin_after_min_floor_stays_in_work_area() {
    // Repro for the `center_main` edge case: a pathological work
    // area smaller than `MIN_WINDOW_*` forces the size to stay at
    // the minimum floor (480×360), which is larger than the work
    // area itself (e.g. 200×150). The naive centered origin would
    // be `(200 - 480)/2 = -140` — title bar off-screen. Running
    // the centered origin through `clamp_to_work_area` must pin it
    // back to the work-area top-left so the title bar is at least
    // reachable.
    let work_area = wa(0, 0, 200, 150);
    let clamped_w = MIN_WINDOW_WIDTH;
    let clamped_h = MIN_WINDOW_HEIGHT;
    let centered_x = work_area.x + (work_area.width as i32 - clamped_w as i32) / 2;
    let centered_y = work_area.y + (work_area.height as i32 - clamped_h as i32) / 2;
    assert!(
        centered_x < work_area.x,
        "precondition: naive center should land off-screen"
    );
    let (x, y, w, h) = clamp_to_work_area(centered_x, centered_y, clamped_w, clamped_h, work_area);
    assert_eq!((x, y), (work_area.x, work_area.y));
    assert_eq!((w, h), (clamped_w, clamped_h));
}
