// Static browser behaviour module for `crates/ui`.
// Runs strictly without `eval` or `new Function` for CSP-restricted hosts.

const activeAsks = new Map();
const activeListeners = new Map();
let nextHandle = 1;

// Active dictation session state (one microphone capture at a time).
let dictationSession = null;

// Active per-element cleanup callbacks for DOM observers/listeners.
const elementBindings = new WeakMap();

function parseArgs(argsJson) {
  if (!argsJson) {
    return [];
  }
  try {
    const parsed = JSON.parse(argsJson);
    return Array.isArray(parsed) ? parsed : [parsed];
  } catch (_) {
    return [];
  }
}

function reportOk(cb, value) {
  try {
    cb(JSON.stringify({ ok: value }));
  } catch (_) {
    // Receiver already dropped on the Rust side.
  }
}

function reportErr(cb, message) {
  try {
    cb(JSON.stringify({ err: String(message ?? "Unknown browser error") }));
  } catch (_) {
    // Receiver already dropped on the Rust side.
  }
}

function prefersReducedMotion() {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

// ---------------------------------------------------------------------------
// Fire-and-forget behaviours (`fires`)
// ---------------------------------------------------------------------------

function clear_file_input(id) {
  if (typeof document === "undefined" || !id) {
    return;
  }
  const el = document.getElementById(id);
  if (el && "value" in el) {
    try {
      el.value = "";
    } catch (_) {
      // Ignore browser restrictions on resetting detached inputs.
    }
  }
}

function dismiss_open_popovers() {
  if (typeof document === "undefined") {
    return;
  }
  const open = document.querySelectorAll(":popover-open");
  for (const el of open) {
    if (typeof el.hidePopover === "function") {
      try {
        el.hidePopover();
      } catch (_) {
        // Ignore already-closing popover.
      }
    }
  }
}

function set_appearance(storageKey, appearance) {
  if (typeof document === "undefined") {
    return;
  }
  const value = appearance === "dark" ? "dark" : "light";
  document.documentElement.dataset.theme = value;
  if (storageKey && typeof window !== "undefined") {
    try {
      window.localStorage.setItem(storageKey, value);
    } catch (_) {
      // Storage may be unavailable in private browsing or restricted contexts.
    }
  }
}

function teardownDictation() {
  const session = dictationSession;
  dictationSession = null;
  if (!session) {
    return;
  }
  if (session.rafId !== null && typeof cancelAnimationFrame === "function") {
    cancelAnimationFrame(session.rafId);
  }
  if (session.intervalId !== null) {
    clearInterval(session.intervalId);
  }
  if (session.onKeyUp && typeof window !== "undefined") {
    window.removeEventListener("keyup", session.onKeyUp, true);
  }
  if (session.recorder && session.recorder.state !== "inactive") {
    try {
      session.recorder.stop();
    } catch (_) {
      // Ignore stop error during cancel.
    }
  }
  if (session.stream) {
    for (const track of session.stream.getTracks()) {
      try {
        track.stop();
      } catch (_) {
        // Ignore track stop errors.
      }
    }
  }
  if (session.audioCtx && typeof session.audioCtx.close === "function") {
    session.audioCtx.close().catch(() => {});
  }
}

function cancel_dictation() {
  if (dictationSession) {
    dictationSession.cancelled = true;
  }
  teardownDictation();
}

function show_modal(id) {
  if (typeof document === "undefined" || !id) {
    return;
  }
  const dialog = document.getElementById(id);
  if (!dialog || typeof dialog.showModal !== "function") {
    return;
  }
  if (!dialog.open) {
    try {
      dialog.showModal();
    } catch (_) {
      // Ignore if dialog is detached or already modal.
    }
  }
}

function close_dialog(id) {
  if (typeof document === "undefined" || !id) {
    return;
  }
  const dialog = document.getElementById(id);
  if (!dialog || typeof dialog.close !== "function") {
    return;
  }
  if (dialog.open) {
    try {
      dialog.close();
    } catch (_) {
      // Ignore close errors.
    }
  }
}

function position_popover(popoverId, anchorId, preferAbove) {
  if (typeof document === "undefined" || typeof window === "undefined") {
    return;
  }
  const popover = document.getElementById(popoverId);
  if (!popover) {
    return;
  }
  const anchor =
    (anchorId ? document.getElementById(anchorId) : null) ||
    document.querySelector(`[popovertarget="${CSS.escape(popoverId)}"]`);
  if (!anchor) {
    return;
  }

  const gap = 8;
  const pad = 12;
  const vw = window.innerWidth || 1024;
  const vh = window.innerHeight || 768;
  const anchorRect = anchor.getBoundingClientRect();
  const popRect = popover.getBoundingClientRect();
  const popW = popRect.width || 320;
  const popH = popRect.height || 180;

  // Respect any visible composer dock at the bottom so sources popovers open clear of it.
  const composer = document.querySelector(".composer");
  const bottomLimit = composer
    ? Math.min(vh - pad, composer.getBoundingClientRect().top - gap)
    : vh - pad;

  const isSidePreview = popover.classList.contains("citation-preview-popover");
  if (isSidePreview) {
    const chosenSide = popover.dataset.side || "";
    const roomRight = vw - anchorRect.right - gap - pad;
    const roomLeft = anchorRect.left - gap - pad;
    let side = chosenSide;
    if (!side) {
      side = roomRight >= roomLeft ? "right" : "left";
    }
    if (side === "right" && roomRight < popW && roomLeft > roomRight) {
      side = "left";
    } else if (side === "left" && roomLeft < popW && roomRight > roomLeft) {
      side = "right";
    }
    popover.dataset.side = side;

    let left =
      side === "right"
        ? anchorRect.right + gap
        : anchorRect.left - gap - popW;
    left = Math.max(pad, Math.min(left, vw - pad - popW));
    let top = anchorRect.top + anchorRect.height / 2 - popH / 2;
    top = Math.max(pad, Math.min(top, vh - pad - popH));

    popover.style.position = "fixed";
    popover.style.margin = "0";
    popover.style.left = `${Math.round(left)}px`;
    popover.style.top = `${Math.round(top)}px`;
    return;
  }

  // Vertical popover (menu / sources list / tooltip): open above if low or requested.
  const spaceBelow = bottomLimit - anchorRect.bottom - gap;
  const spaceAbove = anchorRect.top - pad - gap;
  const openAbove = Boolean(preferAbove) || (spaceBelow < popH && spaceAbove > spaceBelow);

  let top = openAbove
    ? anchorRect.top - gap - popH
    : anchorRect.bottom + gap;
  top = Math.max(pad, Math.min(top, bottomLimit - popH));
  if (top < pad) {
    top = pad;
  }

  let left = anchorRect.left;
  if (left + popW > vw - pad) {
    left = anchorRect.right - popW;
  }
  left = Math.max(pad, Math.min(left, vw - pad - popW));

  popover.dataset.placement = openAbove ? "above" : "below";
  popover.style.position = "fixed";
  popover.style.margin = "0";
  popover.style.left = `${Math.round(left)}px`;
  popover.style.top = `${Math.round(top)}px`;
}

function scroll_to_mark(containerSelector) {
  if (typeof document === "undefined") {
    return;
  }
  const root = containerSelector
    ? document.querySelector(containerSelector)
    : document.querySelector(".source-reading-pane");
  if (!root) {
    return;
  }
  const firstMark = root.querySelector("mark");
  if (!firstMark || typeof firstMark.scrollIntoView !== "function") {
    return;
  }
  firstMark.scrollIntoView({
    block: "center",
    inline: "nearest",
    behavior: prefersReducedMotion() ? "auto" : "smooth",
  });
}

function scroll_region_by_key(id, key) {
  if (typeof document === "undefined" || !id) {
    return;
  }
  const el = document.getElementById(id);
  if (!el) {
    return;
  }
  const step = 48;
  const page = Math.max(step, Math.round(el.clientHeight * 0.8));
  switch (key) {
    case "ArrowDown":
      el.scrollTop += step;
      break;
    case "ArrowUp":
      el.scrollTop -= step;
      break;
    case "ArrowRight":
      el.scrollLeft += step;
      break;
    case "ArrowLeft":
      el.scrollLeft -= step;
      break;
    case "PageDown":
      el.scrollTop += page;
      break;
    case "PageUp":
      el.scrollTop -= page;
      break;
    case "Home":
      el.scrollTop = 0;
      el.scrollLeft = 0;
      break;
    case "End":
      el.scrollTop = el.scrollHeight;
      break;
    default:
      break;
  }
}

function scroll_conversation_bottom(id, instant) {
  if (typeof document === "undefined" || !id) {
    return;
  }
  const el = document.getElementById(id);
  if (!el) {
    return;
  }
  const state = elementBindings.get(el);
  if (state) {
    state.following = true;
    state.userDetached = false;
  }
  const behavior = instant || prefersReducedMotion() ? "auto" : "smooth";
  el.scrollTo({ top: el.scrollHeight, behavior });
}

function positionSelectionPill(host, animate) {
  const active = host.querySelector(
    '[aria-pressed="true"], [aria-current="page"], [aria-selected="true"], [data-selected="true"]'
  );
  const pill = host.querySelector(".selection-pill");
  if (!active || !pill) {
    return;
  }
  const hostRect = host.getBoundingClientRect();
  const targetRect = active.getBoundingClientRect();
  if (hostRect.width === 0 || targetRect.width === 0) {
    return;
  }
  const targetX = targetRect.left - hostRect.left + host.scrollLeft;
  const targetY = targetRect.top - hostRect.top + host.scrollTop;
  const targetW = targetRect.width;
  const targetH = targetRect.height;

  // Capture current computed visual rect so an in-flight transition continues from where it is.
  const currentRect = pill.getBoundingClientRect();
  const hasPrev = pill.dataset.positioned === "true" && currentRect.width > 0;
  if (animate && hasPrev && !prefersReducedMotion()) {
    const startX = currentRect.left - hostRect.left + host.scrollLeft;
    const startY = currentRect.top - hostRect.top + host.scrollTop;
    pill.style.transition = "none";
    pill.style.transform = `translate3d(${startX}px, ${startY}px, 0)`;
    pill.style.width = `${currentRect.width}px`;
    pill.style.height = `${currentRect.height}px`;
    void pill.offsetWidth;
    pill.style.transition = "";
  } else {
    pill.style.transition = "none";
  }

  pill.style.opacity = "1";
  pill.style.transform = `translate3d(${targetX}px, ${targetY}px, 0)`;
  pill.style.width = `${targetW}px`;
  pill.style.height = `${targetH}px`;
  pill.dataset.positioned = "true";
}

function move_selection_pill(hostId) {
  if (typeof document === "undefined" || !hostId) {
    return;
  }
  const host = document.getElementById(hostId);
  if (!host) {
    return;
  }
  positionSelectionPill(host, true);
  if (!elementBindings.has(host) && typeof ResizeObserver === "function") {
    const ro = new ResizeObserver(() => positionSelectionPill(host, false));
    ro.observe(host);
    elementBindings.set(host, { ro });
  }
}

function sync_history_depth(delta) {
  if (typeof window === "undefined" || !window.history) {
    return;
  }
  const step = Number(delta) || 0;
  if (step > 0) {
    for (let i = 0; i < step; i += 1) {
      const nextDepth = ((window.history.state && Number(window.history.state.uiDepth)) || 0) + 1;
      try {
        window.history.pushState({ uiDepth: nextDepth }, "");
      } catch (_) {
        break;
      }
    }
  } else if (step < 0) {
    try {
      window.history.go(step);
    } catch (_) {
      // Ignore history errors in restricted origins.
    }
  }
}

function focus_element(selector) {
  if (typeof document === "undefined" || !selector) {
    return;
  }
  const el = document.querySelector(selector);
  if (el && typeof el.focus === "function") {
    el.focus();
  }
}

function bind_sheet_drag(dialogId) {
  if (typeof document === "undefined" || !dialogId) {
    return;
  }
  const dialog = document.getElementById(dialogId);
  if (!dialog || elementBindings.has(dialog)) {
    return;
  }
  let startY = 0;
  let dragging = false;
  let offset = 0;

  const onPointerDown = (ev) => {
    const handle = ev.target && ev.target.closest ? ev.target.closest(".dialog-sheet-handle, .dialog-header") : null;
    if (!handle) {
      return;
    }
    dragging = true;
    startY = ev.clientY;
    offset = 0;
    dialog.style.transition = "none";
    if (typeof dialog.setPointerCapture === "function") {
      try {
        dialog.setPointerCapture(ev.pointerId);
      } catch (_) {
        // Ignore synthetic pointer ids in tests.
      }
    }
  };

  const onPointerMove = (ev) => {
    if (!dragging) {
      return;
    }
    offset = Math.max(0, ev.clientY - startY);
    dialog.style.transform = `translateY(${offset}px)`;
  };

  const finishDrag = () => {
    if (!dragging) {
      return;
    }
    dragging = false;
    dialog.style.transition = "";
    const threshold = Math.max(96, (dialog.getBoundingClientRect().height || 300) * 0.3);
    if (offset >= threshold) {
      dialog.style.transform = "";
      dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
      if (dialog.open && typeof dialog.close === "function") {
        dialog.close();
      }
    } else {
      dialog.style.transform = "translateY(0)";
    }
    offset = 0;
  };

  dialog.addEventListener("pointerdown", onPointerDown);
  dialog.addEventListener("pointermove", onPointerMove);
  dialog.addEventListener("pointerup", finishDrag);
  dialog.addEventListener("pointercancel", finishDrag);
  elementBindings.set(dialog, { bound: true });
}

function bind_menu_keys(menuId) {
  if (typeof document === "undefined" || !menuId) {
    return;
  }
  const menu = document.getElementById(menuId);
  if (!menu || elementBindings.has(menu)) {
    return;
  }

  const enabledItems = () =>
    Array.from(menu.querySelectorAll('[role="menuitem"]')).filter(
      (item) => !item.disabled && item.getAttribute("aria-disabled") !== "true"
    );

  const onToggle = (ev) => {
    if (ev.newState === "open") {
      position_popover(menuId, null, false);
      const first = enabledItems()[0];
      if (first && typeof first.focus === "function") {
        // Keep keyboard focus ready on the menu container so first ArrowDown focuses item 0.
        menu.focus({ preventScroll: true });
      }
    }
  };

  const onKeyDown = (ev) => {
    if (!menu.matches(":popover-open")) {
      return;
    }
    const items = enabledItems();
    if (ev.key === "Escape") {
      ev.preventDefault();
      if (typeof menu.hidePopover === "function") {
        menu.hidePopover();
      }
      return;
    }
    if (items.length === 0) {
      return;
    }
    const currentIdx = items.indexOf(document.activeElement);
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      const next = currentIdx < 0 ? 0 : (currentIdx + 1) % items.length;
      items[next].focus();
    } else if (ev.key === "ArrowUp") {
      ev.preventDefault();
      const prev = currentIdx <= 0 ? items.length - 1 : currentIdx - 1;
      items[prev].focus();
    } else if (ev.key === "Home") {
      ev.preventDefault();
      items[0].focus();
    } else if (ev.key === "End") {
      ev.preventDefault();
      items[items.length - 1].focus();
    }
  };

  const onClick = (ev) => {
    const item = ev.target && ev.target.closest ? ev.target.closest('[role="menuitem"]') : null;
    if (item && !item.disabled && item.getAttribute("aria-disabled") !== "true") {
      if (typeof menu.hidePopover === "function") {
        menu.hidePopover();
      }
    }
  };

  menu.addEventListener("toggle", onToggle);
  document.addEventListener("keydown", onKeyDown);
  menu.addEventListener("click", onClick);
  elementBindings.set(menu, { onToggle, onKeyDown, onClick });
}

function open_answer_source(answerId, number) {
  if (typeof document === "undefined" || !answerId) {
    return;
  }
  const root = document.getElementById(answerId);
  if (!root) {
    return;
  }
  const key = String(number);
  const direct = root.querySelector(
    `button.citation-chip[data-citation-index="${key}"]`,
  );
  if (direct instanceof HTMLElement) {
    direct.click();
    return;
  }
  const fallback = [...root.querySelectorAll("button.citation-chip")].find(
    (btn) => btn.textContent && btn.textContent.trim() === key,
  );
  if (fallback instanceof HTMLElement) {
    fallback.click();
  }
}

function bind_conversation_follow(viewportId) {
  if (typeof document === "undefined" || !viewportId) {
    return;
  }
  const el = document.getElementById(viewportId);
  if (!el || elementBindings.has(el)) {
    return;
  }
  const state = {
    following: true,
    userDetached: false,
    pointerDown: false,
    lastScrollTop: el.scrollTop,
    lastScrollHeight: el.scrollHeight,
  };

  const hasSelectionInside = () => {
    const sel = typeof window !== "undefined" && window.getSelection ? window.getSelection() : null;
    if (!sel || sel.isCollapsed) {
      return false;
    }
    const anchor = sel.anchorNode;
    const focus = sel.focusNode;
    return (anchor && el.contains(anchor)) || (focus && el.contains(focus));
  };

  const atBottom = () => el.scrollHeight - el.clientHeight - el.scrollTop <= 24;

  const syncFollowAttr = () => {
    el.dataset.following = state.following ? "true" : "false";
    const jump = el.parentElement ? el.parentElement.querySelector(".conversation-jump") : null;
    if (jump) {
      jump.hidden = state.following;
    }
  };

  const onScroll = () => {
    const curTop = el.scrollTop;
    const grew = el.scrollHeight > state.lastScrollHeight;
    if (curTop < state.lastScrollTop - 2 && !grew) {
      state.following = false;
      state.userDetached = true;
    } else if (atBottom() && !state.pointerDown) {
      state.following = true;
      state.userDetached = false;
    } else if (!atBottom()) {
      state.following = false;
    }
    state.lastScrollTop = curTop;
    state.lastScrollHeight = el.scrollHeight;
    syncFollowAttr();
  };

  const onPointerDown = () => {
    state.pointerDown = true;
  };
  const onPointerUp = () => {
    state.pointerDown = false;
    if (atBottom()) {
      state.following = true;
      state.userDetached = false;
      syncFollowAttr();
    }
  };

  const scrollOnGrow = () => {
    const prevHeight = state.lastScrollHeight;
    const nextHeight = el.scrollHeight;
    state.lastScrollHeight = nextHeight;
    if (!state.following || state.userDetached || state.pointerDown || hasSelectionInside()) {
      syncFollowAttr();
      return;
    }
    if (nextHeight <= prevHeight) {
      return;
    }
    // If a single reply jumps taller than one viewport, land at its top instead of scrolling past its opening.
    const delta = nextHeight - prevHeight;
    const targetTop =
      delta > el.clientHeight * 0.9
        ? Math.max(0, prevHeight - 24)
        : nextHeight;
    el.scrollTo({
      top: targetTop,
      behavior: prefersReducedMotion() ? "auto" : "smooth",
    });
    state.lastScrollTop = el.scrollTop;
    syncFollowAttr();
  };

  el.addEventListener("scroll", onScroll, { passive: true });
  el.addEventListener("pointerdown", onPointerDown, { passive: true });
  window.addEventListener("pointerup", onPointerUp, { passive: true });

  if (typeof MutationObserver === "function") {
    const mo = new MutationObserver(scrollOnGrow);
    mo.observe(el, { childList: true, subtree: true, characterData: true });
    state.mo = mo;
  }
  if (typeof ResizeObserver === "function") {
    const ro = new ResizeObserver(scrollOnGrow);
    ro.observe(el);
    state.ro = ro;
  }

  elementBindings.set(el, state);
  syncFollowAttr();
}

// ---------------------------------------------------------------------------
// Request-response behaviours (`asks`)
// ---------------------------------------------------------------------------

async function copy_text(text) {
  if (
    typeof navigator === "undefined" ||
    !navigator.clipboard ||
    typeof navigator.clipboard.writeText !== "function"
  ) {
    throw new Error("Clipboard access is unavailable in this browser.");
  }
  try {
    await navigator.clipboard.writeText(String(text ?? ""));
    return true;
  } catch (err) {
    const detail = err && err.message ? err.message : "Clipboard permission was refused.";
    throw new Error(detail);
  }
}

async function read_appearance(storageKey) {
  if (typeof window !== "undefined") {
    if (storageKey) {
      try {
        const stored = window.localStorage.getItem(storageKey);
        if (stored === "dark" || stored === "light") {
          document.documentElement.dataset.theme = stored;
          return stored;
        }
      } catch (_) {
        // Ignore storage read failure.
      }
    }
    if (typeof document !== "undefined") {
      const explicit = document.documentElement.dataset.theme;
      if (explicit === "dark" || explicit === "light") {
        return explicit;
      }
    }
    if (
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches
    ) {
      return "dark";
    }
  }
  return "light";
}

async function stop_dictation() {
  const session = dictationSession;
  if (!session || !session.recorder) {
    throw new Error("No active dictation session.");
  }
  const recorder = session.recorder;
  const mimeType = session.mimeType || recorder.mimeType || "audio/webm";

  const blob = await new Promise((resolve, reject) => {
    const onStop = () => {
      try {
        const merged = new Blob(session.chunks, { type: mimeType });
        resolve(merged);
      } catch (err) {
        reject(err);
      }
    };
    const onError = (ev) => {
      reject(new Error((ev && ev.error && ev.error.message) || "Recording failed."));
    };
    if (recorder.state === "inactive") {
      onStop();
      return;
    }
    recorder.addEventListener("stop", onStop, { once: true });
    recorder.addEventListener("error", onError, { once: true });
    try {
      recorder.stop();
    } catch (err) {
      reject(err);
    }
  });

  teardownDictation();

  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize));
  }
  return {
    audio_base64: btoa(binary),
    mime_type: mimeType,
  };
}

// ---------------------------------------------------------------------------
// Streaming behaviours (`listens`)
// ---------------------------------------------------------------------------

function watch_cover_shortcut(emit) {
  if (typeof window === "undefined") {
    return () => {};
  }
  const onKeyDown = (ev) => {
    if ((ev.metaKey || ev.ctrlKey) && ev.shiftKey && String(ev.key || "").toLowerCase() === "l") {
      ev.preventDefault();
      ev.stopPropagation();
      emit({ ok: true });
    }
  };
  window.addEventListener("keydown", onKeyDown, true);
  return () => window.removeEventListener("keydown", onKeyDown, true);
}

function start_dictation(emit, trigger) {
  teardownDictation();

  if (
    typeof navigator === "undefined" ||
    !navigator.mediaDevices ||
    typeof navigator.mediaDevices.getUserMedia !== "function"
  ) {
    emit({ ok: { kind: "error", message: "Microphone access is unavailable in this browser." } });
    return () => {};
  }

  let disposed = false;
  const session = {
    cancelled: false,
    chunks: [],
    recorder: null,
    stream: null,
    audioCtx: null,
    rafId: null,
    intervalId: null,
    onKeyUp: null,
    mimeType: "audio/webm",
  };
  dictationSession = session;

  if (trigger === "hold_shortcut" && typeof window !== "undefined") {
    session.onKeyUp = (ev) => {
      if (ev.key === " " || ev.key === "Control" || ev.key === "Shift") {
        if (!disposed && !session.cancelled) {
          emit({ ok: { kind: "released" } });
        }
      }
    };
    window.addEventListener("keyup", session.onKeyUp, true);
  }

  navigator.mediaDevices
    .getUserMedia({ audio: true })
    .then((stream) => {
      if (disposed || session.cancelled || dictationSession !== session) {
        for (const t of stream.getTracks()) {
          t.stop();
        }
        return;
      }
      session.stream = stream;
      const mimeType =
        typeof MediaRecorder !== "undefined" &&
        typeof MediaRecorder.isTypeSupported === "function" &&
        MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
          ? "audio/webm;codecs=opus"
          : "audio/webm";
      session.mimeType = mimeType;

      const recorder = new MediaRecorder(stream, mimeType ? { mimeType } : undefined);
      session.recorder = recorder;
      recorder.ondataavailable = (ev) => {
        if (ev && ev.data && ev.data.size > 0) {
          session.chunks.push(ev.data);
        }
      };
      recorder.start(200);
      emit({ ok: { kind: "started" } });

      try {
        const AudioCtx = window.AudioContext || window.webkitAudioContext;
        if (AudioCtx) {
          const ctx = new AudioCtx();
          session.audioCtx = ctx;
          const source = ctx.createMediaStreamSource(stream);
          const analyser = ctx.createAnalyser();
          analyser.fftSize = 256;
          source.connect(analyser);
          const data = new Uint8Array(analyser.frequencyBinCount);
          session.intervalId = setInterval(() => {
            if (disposed || session.cancelled) {
              return;
            }
            analyser.getByteTimeDomainData(data);
            let sum = 0;
            for (let i = 0; i < data.length; i += 1) {
              const norm = (data[i] - 128) / 128;
              sum += norm * norm;
            }
            const rms = Math.min(1, Math.sqrt(sum / data.length) * 3);
            emit({ ok: { kind: "level", level: rms } });
          }, 100);
        }
      } catch (_) {
        // Waveform metering is best-effort; dictation still proceeds.
      }
    })
    .catch((err) => {
      if (!disposed && !session.cancelled) {
        emit({
          ok: {
            kind: "error",
            message: (err && err.message) || "Microphone access was refused.",
          },
        });
      }
      if (dictationSession === session) {
        teardownDictation();
      }
    });

  return () => {
    disposed = true;
    if (dictationSession === session) {
      teardownDictation();
    }
  };
}

function watch_popstate(emit) {
  if (typeof window === "undefined") {
    return () => {};
  }
  const onPopState = (ev) => {
    const depth = (ev.state && Number(ev.state.uiDepth)) || 0;
    emit({ ok: depth });
  };
  window.addEventListener("popstate", onPopState);
  return () => window.removeEventListener("popstate", onPopState);
}

function watch_popover_page(emit, popoverId, anchorId) {
  if (typeof window === "undefined" || typeof document === "undefined") {
    return () => {};
  }
  const update = () => {
    position_popover(popoverId, anchorId, false);
    emit({ ok: { kind: "repositioned" } });
  };
  const onKeyDown = (ev) => {
    if (ev.key === "Escape") {
      emit({ ok: { kind: "dismissed" } });
    }
  };
  window.addEventListener("scroll", update, true);
  window.addEventListener("resize", update, { passive: true });
  window.addEventListener("keydown", onKeyDown, true);
  update();
  return () => {
    window.removeEventListener("scroll", update, true);
    window.removeEventListener("resize", update);
    window.removeEventListener("keydown", onKeyDown, true);
  };
}

function watch_conversation_viewport(emit, viewportId) {
  if (typeof document === "undefined" || !viewportId) {
    return () => {};
  }
  const el = document.getElementById(viewportId);
  if (!el) {
    return () => {};
  }
  bind_conversation_follow(viewportId);
  let lastFollowing = true;
  const check = () => {
    const following = el.dataset.following !== "false";
    if (following !== lastFollowing) {
      lastFollowing = following;
      emit({ ok: following });
    }
  };
  el.addEventListener("scroll", check, { passive: true });
  const mo = typeof MutationObserver === "function" ? new MutationObserver(check) : null;
  if (mo) {
    mo.observe(el, { attributes: true, attributeFilter: ["data-following"] });
  }
  return () => {
    el.removeEventListener("scroll", check);
    if (mo) {
      mo.disconnect();
    }
  };
}

// ---------------------------------------------------------------------------
// Exact 1:1 dispatch tables verified by `crates/boundaries`
// ---------------------------------------------------------------------------

const fires = {
  clear_file_input,
  dismiss_open_popovers,
  set_appearance,
  cancel_dictation,
  show_modal,
  close_dialog,
  position_popover,
  scroll_to_mark,
  scroll_region_by_key,
  scroll_conversation_bottom,
  move_selection_pill,
  sync_history_depth,
  focus_element,
  bind_sheet_drag,
  bind_menu_keys,
  bind_conversation_follow,
  open_answer_source,
};

const asks = {
  copy_text,
  read_appearance,
  stop_dictation,
};

const listens = {
  watch_cover_shortcut,
  start_dictation,
  watch_popstate,
  watch_popover_page,
  watch_conversation_viewport,
};

// ---------------------------------------------------------------------------
// Exported wasm-bindgen entry points
// ---------------------------------------------------------------------------

export function fire(name, argsJson) {
  const fn = fires[name];
  if (typeof fn !== "function") {
    return;
  }
  const args = parseArgs(argsJson);
  try {
    fn(...args);
  } catch (_) {
    // Fire-and-forget actions never throw unhandled rejections across wasm boundary.
  }
}

export function ask(name, argsJson, onReport) {
  const handle = nextHandle++;
  const entry = { cancelled: false, onReport };
  activeAsks.set(handle, entry);

  const fn = asks[name];
  if (typeof fn !== "function") {
    activeAsks.delete(handle);
    reportErr(onReport, `Unknown ask behaviour: ${name}`);
    return handle;
  }

  const args = parseArgs(argsJson);
  Promise.resolve()
    .then(() => fn(...args))
    .then((value) => {
      if (!entry.cancelled && activeAsks.delete(handle)) {
        reportOk(entry.onReport, value);
      }
    })
    .catch((err) => {
      if (!entry.cancelled && activeAsks.delete(handle)) {
        reportErr(entry.onReport, err && err.message ? err.message : String(err));
      }
    });

  return handle;
}

export function listen(name, argsJson, onReport) {
  const handle = nextHandle++;
  const entry = { cancelled: false, onReport, cleanup: null };
  activeListeners.set(handle, entry);

  const fn = listens[name];
  if (typeof fn !== "function") {
    activeListeners.delete(handle);
    reportErr(onReport, `Unknown listen behaviour: ${name}`);
    return handle;
  }

  const emit = (payload) => {
    if (entry.cancelled || !activeListeners.has(handle)) {
      return;
    }
    try {
      entry.onReport(JSON.stringify(payload));
    } catch (_) {
      cancel_listen(handle);
    }
  };

  const args = parseArgs(argsJson);
  try {
    const cleanup = fn(emit, ...args);
    if (typeof cleanup === "function") {
      entry.cleanup = cleanup;
    }
  } catch (err) {
    activeListeners.delete(handle);
    reportErr(onReport, err && err.message ? err.message : String(err));
  }

  return handle;
}

export function cancel_ask(handle) {
  const entry = activeAsks.get(handle);
  if (entry) {
    entry.cancelled = true;
    activeAsks.delete(handle);
  }
}

export function cancel_listen(handle) {
  const entry = activeListeners.get(handle);
  if (!entry) {
    return;
  }
  entry.cancelled = true;
  activeListeners.delete(handle);
  if (typeof entry.cleanup === "function") {
    try {
      entry.cleanup();
    } catch (_) {
      // Ignore cleanup errors.
    }
  }
}
