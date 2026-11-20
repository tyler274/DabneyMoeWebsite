// The crate's browser behaviour, each piece a function called by name, as `src/script.rs` declares
// it. A browser build imports this file as a module, so a page that refuses `eval`, such as a Chrome
// extension's, runs it; a native webview imports it once per page. A function either only acts; or
// answers once, by returning its answer or a promise of it; or keeps listening, reporting through
// `send` and returning how to stop.

// Marks the root with the phone `platform` the build runs on (`data-platform`, ios or android),
// which decides the form of the motion, or with none when it is null. Installs, once per page:
// which way the person is driving it (`data-interaction` on the root, pointer or keyboard), which
// decides where a focus ring shows; `window.codaMotion` for motion
// measured before and after, which runs whichever way the page is driven and only reduced motion
// stops; and the slide of every selection host's fill when its current item changes.
function motion(platform) {
  const root = document.documentElement;
  if (platform !== null) root.dataset.platform = platform;
  if (window.codaMotion) return;
  const pointer = () => { if (root.dataset.interaction !== "pointer") root.dataset.interaction = "pointer"; };
  document.addEventListener("pointerdown", pointer, true);
  document.addEventListener("pointermove", pointer, { capture: true, passive: true });
  document.addEventListener("keydown", () => { root.dataset.interaction = "keyboard"; }, true);
  const reduced = matchMedia("(prefers-reduced-motion: reduce)");
  const token = (name) => getComputedStyle(root).getPropertyValue(name).trim();
  const milliseconds = (value) => parseFloat(value) * (value.endsWith("ms") ? 1 : 1000);
  // An element animated on a motion role, or not at all under reduced motion.
  const animate = (element, keyframes, role, easing, pseudoElement) => reduced.matches ? undefined
    : element.animate(keyframes, { duration: milliseconds(token(`--motion-${role}`)), easing: token(`--ease-${easing}`),
      ...(pseudoElement === undefined ? {} : { pseudoElement }) });
  // The item a selection host's fill is anchored to (components.css).
  const currentItem = (host) => host.querySelector('[aria-current]:not([aria-current="false"])');
  // Only when the selection itself moves does the fill travel, from where it is drawn: on the item it
  // left, or, while a slide to that item is still under way, as far along as the slide has carried it.
  // That slide stops there, so the new one goes on from the same place.
  const anchored = CSS.supports("anchor-name: --selection");
  const shown = new WeakMap();
  const slide = "selection-slide";
  const drawn = (host, item) => {
    const rest = item.getBoundingClientRect();
    const sliding = host.getAnimations({ subtree: true }).find((running) => running.id === slide && running.effect.target === host);
    if (sliding === undefined) return rest;
    const progress = sliding.effect.getComputedTiming().progress ?? 1;
    const start = sliding.effect.getKeyframes()[0];
    sliding.cancel();
    const [x, y = 0] = start.translate.split(" ").map(parseFloat);
    const along = (from, to) => from + (to - from) * progress;
    return { left: along(rest.left + x, rest.left), top: along(rest.top + y, rest.top),
      width: along(parseFloat(start.width), rest.width), height: along(parseFloat(start.height), rest.height) };
  };
  const follow = () => {
    for (const host of document.querySelectorAll(".selection-host")) {
      const next = currentItem(host);
      const known = shown.has(host);
      const from = shown.get(host);
      shown.set(host, next);
      if (!known || from === next || !anchored || !from || !next || !from.isConnected || !host.contains(from)) continue;
      const was = drawn(host, from);
      const is = next.getBoundingClientRect();
      const moving = animate(host, [{ translate: `${was.left - is.left}px ${was.top - is.top}px`, width: `${was.width}px`,
        height: `${was.height}px` }, {}], "step", "emphasized", "::after");
      if (moving !== undefined) moving.id = slide;
    }
  };
  new MutationObserver(follow).observe(root, { subtree: true, childList: true, attributes: true, attributeFilter: ["aria-current"] });
  follow();
  window.codaMotion = { animate, currentItem };
}

// Installs, once per page, every menu's keys, its focus on opening and its closing on a choice.
// The listeners sit on the document, so a menu added later needs nothing more.
function menuKeys() {
  if (!window.codaMenuKeys) {
    window.codaMenuKeys = true;
    const choices = (menu) => [...menu.querySelectorAll('[role^="menuitem"]')];
    const menuOf = (element) => element?.closest?.(".menu:popover-open");
    document.addEventListener("toggle", (event) => {
      // Opened from the keys, focus is on the first choice; otherwise on the menu, where the keys still start.
      if (event.target.classList?.contains("menu") && event.newState === "open") {
        const keyboard = document.documentElement.dataset.interaction === "keyboard";
        (keyboard ? choices(event.target)[0] : event.target)?.focus({ preventScroll: true });
      }
    }, true);
    document.addEventListener("keydown", (event) => {
      const menu = menuOf(event.target);
      if (!menu || event.altKey || event.ctrlKey || event.metaKey) return;
      if (event.key === "Tab") { menu.hidePopover(); return; }
      const all = choices(menu);
      const now = all.indexOf(document.activeElement);
      let next = { ArrowDown: now + 1, ArrowUp: now < 0 ? all.length - 1 : now - 1, Home: 0, End: all.length - 1 }[event.key];
      if (next === undefined && event.key.length === 1 && event.key !== " ") {
        const letter = event.key.toLocaleLowerCase();
        const after = [...all.slice(now + 1), ...all.slice(0, now + 1)];
        const found = after.find((choice) => choice.textContent.trim().toLocaleLowerCase().startsWith(letter));
        if (found) next = all.indexOf(found);
      }
      if (next === undefined) return;
      event.preventDefault();
      all[(next + all.length) % all.length]?.focus({ preventScroll: false });
    });
    document.addEventListener("pointermove", (event) => {
      if (event.pointerType !== "mouse" || !menuOf(event.target)) return;
      const choice = event.target.closest('[role^="menuitem"]');
      if (choice && document.activeElement !== choice) choice.focus({ preventScroll: true });
    }, { passive: true });
    document.addEventListener("click", (event) => {
      const choice = event.target.closest?.('[role^="menuitem"]');
      const menu = menuOf(choice);
      if (menu && choice.getAttribute("aria-disabled") !== "true") menu.hidePopover();
    });
  }
}

// Installs, once per page, every tooltip's showing and leaving. A rest of half a second shows one;
// within a moment of another leaving, the next shows at once. The pointer can cross to the tooltip
// and rest on it: leaving the control waits a moment for it.
function tooltips() {
  if (!window.codaTooltips) {
    window.codaTooltips = true;
    let timer, leaving, shown, warmUntil = 0;
    const tipOf = (host) => host?.querySelector(":scope > .tooltip");
    const hide = () => {
      clearTimeout(timer);
      clearTimeout(leaving);
      if (shown?.matches(":popover-open")) { shown.hidePopover(); warmUntil = performance.now() + 400; }
      shown = undefined;
    };
    const show = (host, now) => {
      clearTimeout(timer);
      clearTimeout(leaving);
      const tip = tipOf(host);
      if (!tip || tip === shown) return;
      // It opens on a task of its own, never inside a focus change another popover is making.
      const open = () => { hide(); if (tip.isConnected) { tip.showPopover(); shown = tip; } };
      timer = setTimeout(open, now || shown || performance.now() < warmUntil ? 0 : 500);
    };
    document.addEventListener("pointerover", (event) => {
      if (event.pointerType !== "mouse") return;
      const host = event.target.closest?.(".tooltip-host");
      if (host && !host.contains(event.relatedTarget)) show(host, false);
    });
    document.addEventListener("pointerout", (event) => {
      const host = event.target.closest?.(".tooltip-host");
      if (!host || host.contains(event.relatedTarget)) return;
      clearTimeout(timer);
      clearTimeout(leaving);
      leaving = setTimeout(hide, 120);
    });
    document.addEventListener("focusin", (event) => {
      const host = event.target.closest?.(".tooltip-host");
      if (host && event.target.matches(":focus-visible")) show(host, true);
    });
    document.addEventListener("focusout", hide);
    document.addEventListener("pointerdown", hide, true);
    document.addEventListener("keydown", (event) => { if (event.key === "Escape") hide(); }, true);
  }
}

// One context menu host's ways in. `hostId` is its id; its menu's is its `data-menu`, read each
// time it opens. A menu opened at a point keeps that place until it next opens from its own
// control. Lifting uses one layer per page, `window.codaLift`, which blurs the page beneath; its
// motion runs on the springs and simply arrives under reduced motion.
function contextHost(hostId) {
  const host = document.getElementById(hostId);
  if (!host || host.dataset.contextMenu) return;
  host.dataset.contextMenu = "";
  const root = document.documentElement;
  let gone = false;
  // Whether a button or finger is down anywhere on the page, kept once for every host.
  if (!window.codaPressed) {
    window.codaPressed = { down: false };
    addEventListener("pointerdown", () => { window.codaPressed.down = true; }, { capture: true, passive: true });
    for (const ending of ["pointerup", "pointercancel"]) addEventListener(ending, () => { window.codaPressed.down = false; }, { capture: true, passive: true });
  }
  const margin = 12, gap = 8;
  const immediate = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
  const timing = (name) => {
    const [length, ...curve] = getComputedStyle(root).getPropertyValue(name).trim().split(" ");
    return { duration: parseFloat(length) * (length.endsWith("ms") ? 1 : 1000), easing: curve.join(" ") || "ease-out" };
  };
  // A press released outside a popover closes every open one, so a menu asked for while a button or finger is still down
  // opens once it is let go.
  const released = (then) => {
    if (!window.codaPressed.down) { then(); return; }
    const go = () => { removeEventListener("pointerup", go, true); removeEventListener("pointercancel", go, true); setTimeout(() => { if (!gone) then(); }); };
    addEventListener("pointerup", go, true);
    addEventListener("pointercancel", go, true);
  };
  const run = (element, keyframes, how) => immediate() ? undefined : element.animate(keyframes, how);
  // Shows the menu at a point, its own control's placement given back the next time that control opens it.
  const show = () => {
    const menu = document.getElementById(host.dataset.menu);
    if (!menu) return;
    if (menu.matches(":popover-open")) menu.hidePopover();
    if (menu.dataset.placesItself === undefined) {
      menu.dataset.placesItself = "";
      menu.addEventListener("beforetoggle", (event) => {
        if (event.newState !== "open" || menu.dataset.atPoint !== undefined) return;
        delete menu.dataset.openedBy;
        menu.classList.remove("is-at-point");
        for (const property of ["left", "top", "transform-origin"]) menu.style.removeProperty(property);
      });
    }
    menu.classList.add("is-at-point");
    menu.dataset.openedBy = host.id;
    menu.dataset.atPoint = "";
    menu.showPopover();
    delete menu.dataset.atPoint;
    return menu;
  };
  const place = (menu, x, y, leading) => {
    const width = menu.offsetWidth, height = menu.offsetHeight;
    const left = Math.max(margin, Math.min(leading ? x : x - width, innerWidth - margin - width));
    const below = y + height + margin <= innerHeight;
    Object.assign(menu.style, { left: `${left}px`, top: `${below ? y : Math.max(margin, y - height)}px`,
      transformOrigin: `${leading ? "left" : "right"} ${below ? "top" : "bottom"}` });
  };
  const openAt = (x, y) => released(() => { const menu = show(); if (menu) place(menu, x, y, true); });
  // Lifting: the copy rises in place while the finger holds, then, once it is let go, moves only as far as the menu
  // beneath it needs.
  const lift = () => {
    getSelection()?.removeAllRanges();
    const content = host.firstElementChild ?? host;
    // Its own box, without the shrink of the press it may still be settling from.
    const box = content.getBoundingClientRect();
    const at = { width: content.offsetWidth, height: content.offsetHeight };
    at.left = box.left + (box.width - at.width) / 2;
    at.top = box.top + (box.height - at.height) / 2;
    at.right = at.left + at.width;
    const copy = content.cloneNode(true);
    for (const element of [copy, ...copy.querySelectorAll("[id]")]) element.removeAttribute("id");
    copy.inert = true;
    // The copy keeps the box the content was given, never one it would take from the layer.
    Object.assign(copy.style, { boxSizing: "border-box", width: `${at.width}px`, maxWidth: "none", margin: "0" });
    const layer = window.codaLift ??= Object.assign(document.createElement("div"), { className: "context-lift", popover: "manual" });
    layer.dataset.host = host.id;
    if (!layer.isConnected) document.body.append(layer);
    const preview = Object.assign(document.createElement("div"), { className: "context-lift-preview" });
    preview.append(copy);
    layer.replaceChildren(preview);
    layer.classList.remove("is-settling");
    const height = Math.min(at.height, innerHeight * 0.45);
    preview.classList.toggle("is-clipped", at.height > height);
    Object.assign(preview.style, { left: `${at.left}px`, top: `${at.top}px`, width: `${at.width}px`, height: `${height}px` });
    layer.showPopover();
    host.dataset.lifted = "";
    run(preview, [{ scale: 0.97 }, { scale: 1 }], timing("--spring-step"));
    const settle = (to) => {
      layer.classList.add("is-settling");
      const back = run(preview, [{ translate: "0 0" }, { translate: to }], { duration: timing("--motion-control").duration, easing: "ease-in", fill: "forwards" });
      const done = () => { if (layer.contains(preview)) { layer.hidePopover(); layer.replaceChildren(); } delete host.dataset.lifted; };
      back ? back.finished.then(done, done) : done();
    };
    released(() => {
      const menu = show();
      if (!menu) { settle("0 0"); return; }
      const top = Math.max(margin, Math.min(at.top, innerHeight - margin - menu.offsetHeight - gap - height));
      preview.style.top = `${top}px`;
      const leading = at.left + at.width / 2 <= innerWidth / 2;
      place(menu, leading ? at.left : at.right, top + height + gap, leading);
      const away = `0 ${at.top - top}px`;
      run(preview, [{ translate: away }, { translate: "0 0" }], timing("--spring-step"));
      const closed = (event) => {
        if (event.newState !== "closed") return;
        menu.removeEventListener("toggle", closed);
        settle(away);
      };
      menu.addEventListener("toggle", closed);
    });
  };
  // A held press on touch or pen: the content shrinks a touch while held and, unmoved for half a second, lifts; the tap
  // that ends it does nothing else. Movement, lifting the finger or a scroll lets go. Android's own long-press menu
  // request lifts it at once.
  let timer, start, fired = false, touch = false;
  const release = () => { clearTimeout(timer); timer = undefined; delete host.dataset.pressing; removeEventListener("scroll", release, true); };
  const fire = () => { release(); fired = true; lift(); };
  host.addEventListener("pointerdown", (event) => {
    touch = event.pointerType !== "mouse";
    if (!touch || event.button !== 0) return;
    start = [event.clientX, event.clientY];
    fired = false;
    host.dataset.pressing = "";
    addEventListener("scroll", release, { capture: true, passive: true });
    timer = setTimeout(fire, 500);
  });
  host.addEventListener("pointermove", (event) => {
    if (timer !== undefined && Math.hypot(event.clientX - start[0], event.clientY - start[1]) > 10) release();
  });
  for (const ending of ["pointerup", "pointercancel", "pointerleave"]) host.addEventListener(ending, release);
  host.addEventListener("click", (event) => {
    if (fired) { fired = false; event.preventDefault(); event.stopPropagation(); }
  }, true);
  host.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    if (touch) { if (!fired) fire(); return; }
    openAt(event.clientX, event.clientY);
  });
  host.addEventListener("keydown", (event) => {
    if (event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
    event.preventDefault();
    const at = (host.contains(document.activeElement) ? document.activeElement : host).getBoundingClientRect();
    openAt(at.left, at.bottom + 4);
  });
  window.codaContextHosts ??= {};
  window.codaContextHosts[host.id] = {
    dispose() {
      gone = true;
      release();
      const layer = window.codaLift;
      if (layer?.dataset.host === host.id) { if (layer.matches(":popover-open")) layer.hidePopover(); layer.replaceChildren(); }
      const menu = document.getElementById(host.dataset.menu);
      if (menu?.dataset.openedBy === host.id && menu.matches(":popover-open")) menu.hidePopover();
      delete window.codaContextHosts[host.id];
    },
  };
}

// Ends context menu host `hostId`: a press or opening still pending never lands, and its lifted
// copy, and a menu it opened, close.
function disposeContextHost(hostId) {
  window.codaContextHosts?.[hostId]?.dispose();
}

// A list the server reorders: each row it keeps slides from where it stood to where it now stands,
// its current item carrying the selection fill. A row that rises past others is lifted while it
// moves, so it passes over them rather than through them. Rows that only join or leave move nothing
// here; their own frames open and close. The frames of list `id` are its children, in the order of
// `keys`.
function reorder(id, keys) {
  const list = document.getElementById(id);
  if (list !== null) {
    const frames = [...list.children].filter((child) => child.classList.contains("row-frame"));
    const tops = new Map(keys.map((key, index) => [key, frames[index]?.offsetTop ?? 0]));
    const before = list.codaShownRows ?? { keys: [], tops: new Map() };
    list.codaShownRows = { keys, tops };
    const motion = window.codaMotion;
    const kept = keys.filter((key) => before.tops.has(key));
    if (motion !== undefined && kept.join("\n") !== before.keys.filter((key) => tops.has(key)).join("\n")) {
      const selected = motion.currentItem(list);
      keys.forEach((key, index) => {
        const from = before.tops.get(key);
        const to = tops.get(key);
        const frame = frames[index];
        if (from === undefined || frame === undefined || from === to) return;
        const shift = [{ translate: `0 ${from - to}px` }, {}];
        const moving = motion.animate(frame, shift, "step", "emphasized");
        if (moving !== undefined && from > to) {
          frame.classList.add("is-lifted");
          moving.finished.catch(() => undefined).then(() => frame.classList.remove("is-lifted"));
        }
        // The current item carries the fill itself while it moves, so the two never part.
        if (moving !== undefined && selected !== null && frame.contains(selected)) {
          selected.classList.add("is-carrying-selection");
          moving.finished.catch(() => undefined).then(() => selected.classList.remove("is-carrying-selection"));
        }
      });
    }
  }
}

// Answers true once frame `id`'s exit, every running transition of one of its `properties` (a row's
// collapse, a screen's slide and fade), has finished, or at once with none to run, as under reduced
// motion where only fades still run. An exit cancelled by the frame coming back answers false; one
// cancelled while the frame still leaves is watched again.
function watchExit(id, properties) {
  return new Promise((report) => watchFrameExit(id, properties, report));
}

function watchFrameExit(id, properties, report) {
  const watch = () => requestAnimationFrame(() => {
    const frame = document.getElementById(id);
    const exiting = frame ? frame.getAnimations().filter((animation) => properties.includes(animation.transitionProperty)) : [];
    if (exiting.length === 0) { report(true); return; }
    Promise.allSettled(exiting.map((animation) => animation.finished)).then((settled) => {
      if (settled.every((result) => result.status === "fulfilled")) report(true);
      // An exit cancelled while the frame still leaves (motion became immediate) is watched again.
      else if (frame.classList.contains("is-exiting")) watch();
      else report(false);
    });
  });
  watch();
}

// The depth of the history entry the page shows: what the screen stack put under `screenDepth` in its
// state, or 0 for any other state.
function historyDepth() {
  const depth = history.state?.screenDepth;
  return Number.isInteger(depth) && depth > 0 ? depth : 0;
}

// The screen stack's history, one per page: the depth last wanted, whether the stack's own step back
// is still travelling (a step lands later, as a `popstate`), and who hears the person's steps. Its
// one `popstate` listener lives here, for the page's life, not with any watch, so an own step that
// lands after the stack has gone is still taken as its own.
const screenHistory = { wanted: 0, travelling: false, hearing: new Set(), listening: false };

function screenHistoryOwner() {
  if (!screenHistory.listening) {
    screenHistory.listening = true;
    addEventListener("popstate", () => {
      // The first step to land while an own step travels is taken to be that step: it is not
      // reported, and history settles again toward the depth wanted since it set off. If the
      // person's step was queued just before it, the two are swapped, and the person's step is lost
      // or counted against the wrong step.
      if (screenHistory.travelling) {
        screenHistory.travelling = false;
        settleDepth();
        return;
      }
      const depth = historyDepth();
      for (const send of screenHistory.hearing) send(depth);
    });
  }
  return screenHistory;
}

// Makes history hold exactly the depth wanted: one entry pushed for each level missing, or one step
// back over every level too many, which then travels.
function settleDepth() {
  const now = historyDepth();
  const { wanted } = screenHistory;
  for (let level = now + 1; level <= wanted; level += 1) history.pushState({ screenDepth: level }, "");
  if (wanted < now) {
    screenHistory.travelling = true;
    history.go(wanted - now);
  }
}

// Makes history hold exactly `depth` entries above the one the app stands on with no screen open.
// While an own step travels, the depth is only recorded: the step settles toward it as it lands.
function setDepth(depth) {
  const owner = screenHistoryOwner();
  owner.wanted = depth;
  if (!owner.travelling) settleDepth();
}

// Reports the depth of the entry history shows each time the person steps through it, until stopped.
function watchDepth(send) {
  const owner = screenHistoryOwner();
  owner.hearing.add(send);
  return () => owner.hearing.delete(send);
}

// Opens a reading surface's own scrolling body, `bodyId`, at its top, or at the passage or field
// marked in it when the mark would be out of sight there, once the surface has its size: only that
// body moves, never the page beneath. What stands above the record stays in view when it can.
function scrollToPassage(bodyId) {
  requestAnimationFrame(() => requestAnimationFrame(() => {
    const body = document.getElementById(bodyId);
    if (!body) return;
    body.scrollTop = 0;
    const passage = body.querySelector(".source-text mark, .record-fields mark");
    if (!passage) return;
    const shown = body.getBoundingClientRect();
    const at = passage.getBoundingClientRect();
    if (at.bottom > shown.bottom) body.scrollTop += at.top - shown.top;
  }));
}

// Holds the reader's place in conversation `scrollerId` while it is watched, in place of the browser's own anchoring.
// The place is one of two: following the end, or holding the first turn in sight where it stood, whatever changes the
// size of the region or of its content. The turns are the children of its one child.
// The reader's intent decides which: a wheel, finger or key moving back lets go of the end at once, before any of its
// scroll lands, and so does any scroll toward the start that this watch did not make; only reaching the end again by
// moving toward it, a send, or the jump to the newest reply changes that. A reply that arrives while it follows the end,
// and whose top the end would take out of sight, is held instead with its top at the region's top, as the jump holds
// it. While a button or finger is down in the region, such as while words are being selected, following waits, and
// takes up again once it is let go.
// Moving to its place, the region eases, closing the same share of the distance each frame so most of it is covered
// within `--motion-arrive`, and what was in sight moves with new content's own entry instead of jumping; under reduced
// motion it moves at once. A held turn stays where it stood at once, except on the way to the newest reply.
// Reports whether the reader is following the end, now and on each change. Two frames after it starts the region is
// marked settled, so what stood there from the start never runs an entry. Answers how to stop.
function watchReading(scrollerId, send) {
  const scroller = document.getElementById(scrollerId);
  const content = scroller?.firstElementChild;
  if (!scroller || !content) throw new Error(`no conversation ${scrollerId}`);
  let place = { end: true };
  let reported;
  // The position this watch set last, so its own scrolling isn't read as the reader's.
  let ours = null;
  // Where the last scroll left the region, so the next one's direction is known.
  let last = scroller.scrollTop;
  let easing = 0;
  let pressed = false;
  let touchY = null;
  const pace = parseFloat(getComputedStyle(scroller).getPropertyValue('--motion-arrive')) / 4;
  const edge = () => scroller.getBoundingClientRect().top;
  const end = () => scroller.scrollHeight - scroller.clientHeight;
  const set = (top) => { scroller.scrollTop = top; ours = scroller.scrollTop; last = ours; };
  const immediate = () => !('settled' in scroller.dataset) || !(pace > 0) || reducedMotion();
  const stopEasing = () => { cancelAnimationFrame(easing); easing = 0; };
  const report = () => { if (place.end !== reported) { reported = place.end; send(place.end); } };
  // The first turn in sight, held where it stands now.
  const held = () => {
    const top = edge();
    const turn = [...content.children].find((child) => child.getBoundingClientRect().bottom > top);
    return { end: false, turn, offset: turn ? turn.getBoundingClientRect().top - top : 0 };
  };
  // How far the region stands from the reader's place: to the end, or to where the held turn stood.
  const gap = () => place.end ? end() - scroller.scrollTop
    : place.turn?.isConnected ? place.turn.getBoundingClientRect().top - edge() - place.offset : 0;
  const ease = (then) => {
    easing = requestAnimationFrame((now) => {
      easing = 0;
      const distance = gap();
      if (Math.abs(distance) <= 0.5) return;
      // At least a whole pixel a frame: a screen that places scrolling on whole pixels drops a smaller step.
      const step = Math.sign(distance) * Math.min(Math.abs(distance), Math.max(1, Math.abs(distance) * (1 - Math.exp(-(now - then) / pace))));
      const from = scroller.scrollTop;
      set(from + step);
      // Held at the region's edge, it stops; growth that makes room starts it again.
      if (scroller.scrollTop !== from) ease(now);
    });
  };
  // Moves the region toward the reader's place, unless following waits for a press or is already on its way.
  const restore = () => {
    if ((place.end && pressed) || easing) return;
    const distance = gap();
    if (Math.abs(distance) <= 0.5) return;
    if (immediate() || !place.end) set(scroller.scrollTop + distance);
    else ease(performance.now());
  };
  const capture = () => {
    const top = scroller.scrollTop;
    const moved = top - last;
    last = top;
    if (ours !== null && Math.abs(top - ours) <= 1) return;
    ours = null;
    stopEasing();
    const atEnd = end() - top < 1;
    place = (place.end ? atEnd || moved >= 0 : atEnd && moved > 0) ? { end: true } : held();
    report();
  };
  // Letting go of the end, before the move's own scroll lands, so growth in the same frame can't pull the reader back.
  const release = () => {
    stopEasing();
    ours = null;
    if (place.end && scroller.scrollTop > 0) { place = held(); report(); }
  };
  const editing = (target) => target.closest?.('input, textarea, select, [contenteditable]:not([contenteditable="false"])');
  const intent = (event) => {
    if (event.type === 'wheel') { if (event.deltaY < 0) release(); }
    else if (event.type === 'keydown') {
      if (!event.defaultPrevented && !editing(event.target)
        && (['ArrowUp', 'PageUp', 'Home'].includes(event.key) || (event.key === ' ' && event.shiftKey))) release();
    } else if (event.type === 'touchmove') {
      const y = event.touches[0]?.clientY;
      if (y === undefined) return;
      // A finger moving down moves the content toward its start.
      if (touchY !== null && y > touchY) release();
      touchY = y;
    }
  };
  const press = (event) => {
    if (event.type === 'touchstart') touchY = event.touches[0]?.clientY ?? null;
    else if (event.button !== 0 || event.pointerType === 'touch') return;
    pressed = true;
    stopEasing();
  };
  const lift = () => {
    if (!pressed) return;
    pressed = false;
    touchY = null;
    restore();
  };
  const sizes = new ResizeObserver(restore);
  sizes.observe(scroller);
  sizes.observe(content);
  scroller.addEventListener('scroll', capture, { passive: true });
  for (const kind of ['wheel', 'touchmove', 'keydown']) scroller.addEventListener(kind, intent, { passive: true });
  for (const kind of ['pointerdown', 'touchstart']) scroller.addEventListener(kind, press, { passive: true });
  for (const kind of ['pointerup', 'pointercancel', 'touchend', 'touchcancel']) addEventListener(kind, lift, { capture: true, passive: true });
  const newest = () => {
    const replies = scroller.querySelectorAll('.message-assistant');
    return replies[replies.length - 1];
  };
  // Holds `reply`'s turn so the reply's top stands at the region's top, reached the way the end is.
  const holdTop = (reply, smoothly) => {
    stopEasing();
    ours = null;
    const turn = [...content.children].find((child) => child.contains(reply));
    place = { end: false, turn, offset: turn ? turn.getBoundingClientRect().top - reply.getBoundingClientRect().top : 0 };
    report();
    if (smoothly && !immediate()) ease(performance.now());
    else set(scroller.scrollTop + gap());
  };
  scroller.codaReading = {
    toEnd() { place = { end: true }; restore(); report(); },
    jump(smoothly) {
      const reply = newest();
      if (!reply) return;
      holdTop(reply, smoothly);
      if (!reply.hasAttribute('tabindex')) reply.setAttribute('tabindex', '-1');
      reply.focus({ preventScroll: true });
    },
    arrived() {
      const reply = newest();
      if (!place.end || !reply) return;
      // Where the reply's top stands against the region's top now, and where the end would put it.
      const top = reply.getBoundingClientRect().top - edge();
      if (top < 0 || top - (end() - scroller.scrollTop) >= 0) return;
      holdTop(reply, true);
    },
  };
  restore();
  report();
  let settling = requestAnimationFrame(() => { settling = requestAnimationFrame(() => { scroller.dataset.settled = ''; }); });
  return () => {
    cancelAnimationFrame(settling);
    stopEasing();
    sizes.disconnect();
    scroller.removeEventListener('scroll', capture);
    for (const kind of ['wheel', 'touchmove', 'keydown']) scroller.removeEventListener(kind, intent);
    for (const kind of ['pointerdown', 'touchstart']) scroller.removeEventListener(kind, press);
    for (const kind of ['pointerup', 'pointercancel', 'touchend', 'touchcancel']) removeEventListener(kind, lift, { capture: true });
    delete scroller.codaReading;
  };
}

// Brings the reader of conversation `scrollerId` to its end.
function readToEnd(scrollerId) {
  document.getElementById(scrollerId)?.codaReading?.toEnd();
}

// Takes the reader of conversation `scrollerId` to the top of its newest reply, `smoothly` or at once, and focuses it.
function jumpToLatest(scrollerId, smoothly) {
  document.getElementById(scrollerId)?.codaReading?.jump(smoothly);
}

// Shows the reply that just arrived in conversation `scrollerId` from its top, when the reader follows the end and the
// end would take that top out of sight.
function replyArrived(scrollerId) {
  document.getElementById(scrollerId)?.codaReading?.arrived();
}

// What the open preview `previewId` hears from the page while it is open, until stopped: the
// window's size now and on every scroll or resize; Escape, taken first so a surrounding dialog keeps
// it, with whether focus was inside the preview; and citation `citationId` going out of sight,
// scroll areas included. Every Dioxus renderer runs it the same way, so the preview depends on no
// one engine's CSS.
function watchPage(citationId, previewId, send) {
  const root = document.documentElement;
  const citation = document.getElementById(citationId);
  const report = () => send({ kind: 'window', width: root.clientWidth, height: root.clientHeight });
  const escape = (event) => {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    event.stopPropagation();
    // Looked up now: this script starts before the preview is in the page.
    const preview = document.getElementById(previewId);
    send({ kind: 'escape', inside: preview !== null && preview.contains(document.activeElement) });
  };
  const sight = new IntersectionObserver(([entry]) => { if (entry !== undefined && !entry.isIntersecting) send({ kind: 'away' }); });
  if (citation !== null) sight.observe(citation);
  addEventListener('scroll', report, true);
  addEventListener('resize', report);
  document.addEventListener('keydown', escape, true);
  report();
  return () => {
    sight.disconnect();
    removeEventListener('scroll', report, true);
    removeEventListener('resize', report);
    document.removeEventListener('keydown', escape, true);
  };
}

// What popover `popoverId`, which control `controlId` opens, needs to be placed, until stopped: as it
// opens, and on every scroll or resize while it is open, the control's box, the popover's own width
// and the window's size; and that it closed. The browser opens and closes it; the control going out
// of sight, scroll areas included, closes it too.
function watchPopover(controlId, popoverId, send) {
  const root = document.documentElement;
  const control = document.getElementById(controlId);
  const popover = document.getElementById(popoverId);
  if (control === null || popover === null) throw new Error(`no popover ${popoverId} opened by ${controlId}`);
  const report = (kind) => {
    const box = control.getBoundingClientRect();
    send({ kind, control: { x: box.left, y: box.top, width: box.width, height: box.height },
      width: popover.offsetWidth, window: [root.clientWidth, root.clientHeight] });
  };
  const moved = () => report('moved');
  const sight = new IntersectionObserver(([entry]) => {
    if (entry !== undefined && !entry.isIntersecting && popover.matches(':popover-open')) popover.hidePopover();
  });
  const unfollow = () => {
    sight.disconnect();
    removeEventListener('scroll', moved, true);
    removeEventListener('resize', moved);
  };
  const toggled = (event) => {
    if (event.newState === 'open') {
      report('opened');
      sight.observe(control);
      addEventListener('scroll', moved, true);
      addEventListener('resize', moved);
    } else {
      unfollow();
      send({ kind: 'closed' });
    }
  };
  popover.addEventListener('toggle', toggled);
  return () => {
    popover.removeEventListener('toggle', toggled);
    unfollow();
  };
}

// Takes only drags that carry files over composer `id`, which only the browser's own list of what
// a drag holds can tell while it moves. Counted, because entering a child leaves its parent.
function fileDrags(id) {
  const form = document.getElementById(id);
  if (form && !form.dataset.takesFiles) {
    form.dataset.takesFiles = 'true';
    let over = 0;
    const files = (event) => form.querySelector(':scope > .composer-drop') && [...(event.dataTransfer?.types ?? [])].includes('Files');
    const settle = () => { over = 0; delete form.dataset.dropping; };
    form.addEventListener('dragenter', (event) => { if (files(event)) { event.preventDefault(); over += 1; form.dataset.dropping = 'true'; } });
    form.addEventListener('dragover', (event) => { if (files(event)) event.preventDefault(); });
    form.addEventListener('dragleave', (event) => { if (files(event) && --over <= 0) settle(); });
    form.addEventListener('drop', (event) => { if (files(event)) { event.preventDefault(); settle(); } });
  }
}

// Sheet `dialog` follows the finger or pointer down from its handle, its header, or its content
// once that is scrolled to the top. Pulled up, it resists, rubber-banding as iOS does. Let go, it is
// projected where the flick would carry it, decelerating as a native scroll view does: past half
// its height it goes, otherwise it springs back. Either way the release carries the flick's speed
// into the step spring (`--spring-step`: a smooth spring, response 0.35, critically damped).
// Without motion it lands at once. A drag that began on a control doesn't also press it.
function sheetDrag(dialog) {
  const sheet = document.getElementById(dialog);
  const body = sheet && sheet.querySelector('.dialog-body');
  if (sheet && body) {
    const stiffness = (2 * Math.PI / 0.35) ** 2, damping = 4 * Math.PI / 0.35;
    // Where a flick at this speed (px/s) would come to rest, at UIScrollView's normal deceleration.
    const projection = 0.998 / (1 - 0.998) / 1000;
    const still = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
    let drag = null, settling = 0;
    const show = (y) => {
      const height = sheet.offsetHeight || 1;
      sheet.style.transform = `translateY(${y}px)`;
      sheet.style.setProperty('--sheet-shown', String(Math.min(1, Math.max(0, 1 - y / height))));
    };
    const rest = () => { sheet.style.removeProperty('transform'); sheet.style.removeProperty('--sheet-shown'); delete sheet.dataset.dragging; };
    const band = (y, height) => y >= 0 ? y : -(1 - 1 / (-y * 0.55 / height + 1)) * height;
    // Where the sheet is drawn now, whether its entry or a spring is moving it.
    const drawn = () => { const t = getComputedStyle(sheet).transform; return t === 'none' ? 0 : new DOMMatrix(t).m42; };
    // Where the hand would be for a sheet already shown at `y`, so taking hold of it never moves it.
    const unband = (y, height) => y >= 0 ? y : -height * (1 / (1 + y / height) - 1) / 0.55;
    const settle = (from, to, speed, done) => {
      cancelAnimationFrame(settling);
      if (still()) { show(to); done(); return; }
      let x = from, v = speed, last = performance.now();
      const step = (now) => {
        let dt = Math.min(0.032, (now - last) / 1000); last = now;
        while (dt > 0) { const h = Math.min(dt, 1 / 240); v += (-stiffness * (x - to) - damping * v) * h; x += v * h; dt -= h; }
        show(x);
        // The spring is critically damped, so it nears its end without passing it.
        const arrived = to > 0 ? x >= to - 0.5 : Math.abs(x - to) < 0.5 && Math.abs(v) < 5;
        if (arrived) { show(to); done(); } else { settling = requestAnimationFrame(step); }
      };
      settling = requestAnimationFrame(step);
    };
    const typing = (node) => node.closest('input, textarea, select, [contenteditable]') !== null;
    sheet.addEventListener('pointerdown', (event) => {
      if (event.button !== 0 || !sheet.open || sheet.classList.contains('is-closing') || typing(event.target)) return;
      const fromContent = body.contains(event.target) && !event.target.closest('.dialog-header');
      drag = { id: event.pointerId, x: event.clientX, y: event.clientY, fromContent, active: false, from: 0, offset: 0, samples: [[event.timeStamp, event.clientY]] };
    });
    sheet.addEventListener('pointermove', (event) => {
      if (!drag || event.pointerId !== drag.id) return;
      const dx = event.clientX - drag.x, dy = event.clientY - drag.y;
      if (!drag.active) {
        if (Math.abs(dy) < 6 || Math.abs(dy) < Math.abs(dx)) return;
        if (drag.fromContent && (body.scrollTop > 0 || dy < 0)) { drag = null; return; }
        drag.active = true;
        // Taken mid-motion, it is held where it is; the movement that started the drag still counts.
        cancelAnimationFrame(settling);
        drag.from = unband(drawn(), sheet.offsetHeight || 1);
        sheet.dataset.dragging = '';
        sheet.setPointerCapture(event.pointerId);
      }
      const height = sheet.offsetHeight || 1;
      drag.offset = band(drag.from + event.clientY - drag.y, height);
      drag.samples.push([event.timeStamp, event.clientY]);
      while (drag.samples.length > 2 && event.timeStamp - drag.samples[0][0] > 100) drag.samples.shift();
      show(drag.offset);
    });
    // A drag is the sheet's, not the page's: on touch the browser would otherwise scroll it away.
    sheet.addEventListener('touchmove', (event) => { if (drag && drag.active) event.preventDefault(); }, { passive: false });
    const release = (event) => {
      if (!drag || event.pointerId !== drag.id) return;
      const ended = drag; drag = null;
      if (!ended.active) return;
      // The click this release would make belongs to the drag; it arrives before any timer runs.
      const swallow = (click) => { click.preventDefault(); click.stopPropagation(); };
      sheet.addEventListener('click', swallow, { capture: true, once: true });
      setTimeout(() => sheet.removeEventListener('click', swallow, { capture: true }), 0);
      // The speed at release, over its last 100ms: a hand held still before letting go has none.
      const recent = [...ended.samples, [event.timeStamp, event.clientY]].filter(([t]) => event.timeStamp - t <= 100);
      const [first, last] = [recent[0], recent[recent.length - 1]];
      const speed = recent.length > 1 && last[0] > first[0] ? (last[1] - first[1]) / ((last[0] - first[0]) / 1000) : 0;
      const height = sheet.offsetHeight || 1;
      const goes = event.type === 'pointerup' && ended.offset > 0 && ended.offset + speed * projection > height / 2;
      if (goes) {
        settle(ended.offset, height, Math.max(speed, 0), () => sheet.close());
      } else {
        settle(ended.offset, 0, ended.offset < 0 ? 0 : speed, rest);
      }
    };
    sheet.addEventListener('pointerup', release);
    sheet.addEventListener('pointercancel', release);
  }
}

// Moves focus off an arrow of switch `id` that has just been disabled.
function handOff(id) {
  const previous = document.getElementById(`${id}-previous`), next = document.getElementById(`${id}-next`);
  const held = document.activeElement;
  if (held === previous && previous.disabled) next.focus();
  if (held === next && next.disabled) previous.focus();
}

// Opens dialog `id` modally once, focusing its `[autofocus]` or else `focus`; its exit motion
// closes it. Answers true once whatever closes it has, or false if it couldn't open.
function modal(id, focus) {
  const d = document.getElementById(id);
  if (!d || d.open) return false;
  return new Promise((report) => {
    d.addEventListener('animationend', (e) => {
      if (e.target === d && e.animationName === 'dialog-exit') d.close();
    });
    d.addEventListener('close', () => report(true), { once: true });
    d.showModal();
    (d.querySelector('[autofocus]') ?? d.querySelector(focus))?.focus({ preventScroll: true });
  });
}

// Writes `text` to the clipboard; a refusal rejects.
function copy(text) {
  return navigator.clipboard.writeText(text);
}

// Answers whether the person asks for reduced motion.
function reducedMotion() {
  return matchMedia('(prefers-reduced-motion: reduce)').matches;
}

// Sets checkbox `id`'s mixed state, which only a property holds.
function indeterminate(id, mixed) {
  const box = document.getElementById(id);
  if (box) box.indeterminate = mixed;
}

// Shows or hides popover `id`.
function togglePopover(id, show) {
  document.getElementById(id)?.togglePopover(show);
}

// Brings the first citation numbered `number` in answer `answerId` into view, then focuses and
// chooses it, so its source opens as that citation opens it and focus returns to it after.
function openCitation(answerId, number) {
  const citation = document.getElementById(answerId)?.querySelector(`button[data-source-number="${number}"]`);
  if (!citation) return;
  citation.scrollIntoView({ block: 'nearest' });
  citation.focus({ preventScroll: true });
  citation.click();
}

// Scrolls element `id` just into view.
function scrollIntoView(id) {
  document.getElementById(id)?.scrollIntoView({ block: 'nearest' });
}

// Focuses element `id`, scrolling it into view when `reveal` is "scroll" and moving nothing when it
// is "still".
function focusElement(id, reveal) {
  document.getElementById(id)?.focus({ preventScroll: reveal === "still" });
}

// Puts the caret at the end of the focused field.
function caretToEnd() {
  const field = document.activeElement;
  if (field && field.setSelectionRange) field.setSelectionRange(field.value.length, field.value.length);
}

let activeDictation = null;

function stopDictationHardware(state) {
  if (!state) return;
  if (state.timer !== null) {
    clearInterval(state.timer);
    state.timer = null;
  }
  if (state.onKeyUp) {
    globalThis.removeEventListener('keyup', state.onKeyUp, true);
    state.onKeyUp = null;
  }
  const stream = state.stream;
  state.stream = null;
  stream?.getTracks?.().forEach((track) => {
    try { track.stop(); } catch { /* ignore */ }
  });
  const audioCtx = state.audioCtx;
  state.audioCtx = null;
  if (audioCtx?.close) {
    try {
      Promise.resolve(audioCtx.close()).catch(() => {});
    } catch {
      /* ignore */
    }
  }
}

// Starts microphone audio capture and streams normalized loudness levels (`0..1`) until stopped
// or cancelled. When `stopOnKeyRelease` is true, releasing Control, Shift, or Space emits `released`.
function watchDictation(stopOnKeyRelease, send) {
  cancelDictation();
  const nav = globalThis.navigator;
  if (!nav?.mediaDevices?.getUserMedia || typeof MediaRecorder === 'undefined') {
    send({ kind: 'error', message: 'Microphone recording is not supported in this browser.' });
    return () => {};
  }
  const state = {
    cancelled: false,
    stream: null,
    recorder: null,
    audioCtx: null,
    timer: null,
    onKeyUp: null,
    chunks: [],
    mimeType: 'audio/webm',
  };
  if (stopOnKeyRelease) {
    let emitted = false;
    state.onKeyUp = (event) => {
      if (emitted || state.cancelled || activeDictation !== state) return;
      if (event.key === 'Control' || event.key === 'Shift' || event.key === ' ') {
        emitted = true;
        send({ kind: 'released' });
      }
    };
    globalThis.addEventListener('keyup', state.onKeyUp, true);
  }
  activeDictation = state;
  nav.mediaDevices.getUserMedia({ audio: true }).then((stream) => {
    if (state.cancelled || activeDictation !== state) {
      stream.getTracks().forEach((track) => track.stop());
      return;
    }
    state.stream = stream;
    try {
      const recorder = new MediaRecorder(stream);
      state.recorder = recorder;
      state.mimeType = recorder.mimeType || 'audio/webm';
      recorder.ondataavailable = (event) => {
        if (event.data && event.data.size > 0) state.chunks.push(event.data);
      };
      recorder.start(100);
      try {
        const AudioCtx = globalThis.AudioContext || globalThis.webkitAudioContext;
        if (AudioCtx) {
          const audioCtx = new AudioCtx();
          state.audioCtx = audioCtx;
          const source = audioCtx.createMediaStreamSource(stream);
          const analyser = audioCtx.createAnalyser();
          analyser.fftSize = 256;
          source.connect(analyser);
          const samples = new Uint8Array(analyser.frequencyBinCount);
          state.timer = setInterval(() => {
            if (state.cancelled || activeDictation !== state) return;
            analyser.getByteTimeDomainData(samples);
            let sum = 0;
            for (let i = 0; i < samples.length; i += 1) {
              const delta = (samples[i] - 128) / 128;
              sum += delta * delta;
            }
            const rms = Math.min(1, Math.sqrt(sum / samples.length) * 3);
            send({ kind: 'level', level: rms });
          }, 80);
        }
      } catch {
        // Optional loudness meter is unavailable for this audio stream; recording continues.
      }
      send({ kind: 'started' });
    } catch (error) {
      stopDictationHardware(state);
      if (activeDictation === state) activeDictation = null;
      send({ kind: 'error', message: error?.message ?? 'Could not start recording.' });
    }
  }).catch((error) => {
    if (activeDictation === state) activeDictation = null;
    send({ kind: 'error', message: error?.message ?? 'Microphone access was denied.' });
  });
  return () => {
    state.cancelled = true;
    stopDictationHardware(state);
    if (activeDictation === state) activeDictation = null;
  };
}

function bytesToBase64(bytes) {
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

// Stops the active dictation recording and resolves with base64 audio bytes and MIME type.
async function stopDictation() {
  const state = activeDictation;
  activeDictation = null;
  if (!state || !state.recorder) {
    throw new Error('No active dictation session.');
  }
  const recorder = state.recorder;
  if (recorder.state !== 'inactive') {
    await new Promise((resolve) => {
      recorder.addEventListener('stop', resolve, { once: true });
      recorder.stop();
    });
  }
  stopDictationHardware(state);
  const blob = new Blob(state.chunks, { type: state.mimeType });
  const buffer = await blob.arrayBuffer();
  return {
    audio_base64: bytesToBase64(new Uint8Array(buffer)),
    mime_type: blob.type || state.mimeType || 'audio/webm',
  };
}

function cancelDictation() {
  const state = activeDictation;
  activeDictation = null;
  if (!state) return;
  state.cancelled = true;
  if (state.recorder && state.recorder.state !== 'inactive') {
    try { state.recorder.stop(); } catch { /* ignore */ }
  }
  stopDictationHardware(state);
}

const fires = {
  motion, menuKeys, tooltips, contextHost, disposeContextHost, reorder, scrollToPassage, fileDrags,
  sheetDrag, handOff, indeterminate, togglePopover, openCitation, scrollIntoView, focusElement,
  caretToEnd, readToEnd, jumpToLatest, replyArrived, setDepth, cancelDictation,
};
const asks = { watchExit, modal, copy, reducedMotion, stopDictation };
const listens = { watchPage, watchPopover, watchReading, watchDepth, watchDictation };

const running = new Map();
let calls = 0;

const ok = (value) => JSON.stringify({ ok: value ?? null });
const err = (error) => JSON.stringify({ err: `${error?.name ?? 'Error'}: ${error?.message ?? error}` });
// What reaches `report` once Rust has let go of it is nothing.
const telling = (report) => (words) => { try { report(words); } catch { /* Rust let go. */ } };

// Runs behaviour `name` with `args`, a JSON array, for what it does alone.
export function fire(name, args) {
  try {
    fires[name](...JSON.parse(args));
  } catch (error) {
    console.error(`coda ui: ${name}`, error);
  }
}

// Runs behaviour `name` with `args`, a JSON array; `report` hears its answer, or why it threw or
// rejected, exactly once.
export function ask(name, args, report) {
  const tell = telling(report);
  let answer;
  try {
    answer = asks[name](...JSON.parse(args));
  } catch (error) {
    answer = Promise.reject(error);
  }
  Promise.resolve(answer).then((value) => tell(ok(value)), (error) => tell(err(error)));
}

// Runs behaviour `name` with `args`, a JSON array; `report` hears each thing it says, or why it
// threw. Answers the handle `stop` takes.
export function listen(name, args, report) {
  const tell = telling(report);
  try {
    const stops = listens[name](...JSON.parse(args), (value) => tell(ok(value)));
    calls += 1;
    running.set(calls, stops);
    return calls;
  } catch (error) {
    tell(err(error));
    return 0;
  }
}

// Stops the behaviour that call `handle` started.
export function stop(handle) {
  running.get(handle)?.();
  running.delete(handle);
}
