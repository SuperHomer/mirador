// Injected into every browser-pane webview. Exposes window.__miraRun for
// the automation bridge. Rust evaluates it and reads its return value
// (`eval_with_callback`) — the page never gets IPC access to the app, and
// nothing navigates, so a click's own navigation is left alone.
(function () {
  if (window.__miraInit) return;
  window.__miraInit = true;
  let nextId = 0;

  // The page's console, kept for `mira browser console`: this script runs
  // at document start, so it sees errors thrown while the page loads. Per
  // document — a navigation starts an empty one. The cap keeps a page that
  // logs in a loop well under the answer's 1 MB.
  const MAX_ENTRIES = 500;
  const MAX_MESSAGE = 2000;
  let entries = [];
  let dropped = 0;

  function keep(level, message, source) {
    const entry = {
      level,
      message: message.length > MAX_MESSAGE ? message.slice(0, MAX_MESSAGE) + "…" : message,
      time: new Date().toISOString().slice(11, 23),
    };
    if (source) entry.source = source;
    entries.push(entry);
    if (entries.length > MAX_ENTRIES) {
      entries.shift();
      dropped++;
    }
  }

  function describe(value) {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.name + ": " + value.message;
    try {
      const json = JSON.stringify(value);
      if (json !== undefined) return json;
    } catch {}
    return String(value);
  }

  // `file:line:col`, the file by its last path segment, as a person names it.
  function sourceOf(url, line, col) {
    if (!url) return undefined;
    const file = url.split(/[?#]/)[0].split("/").pop() || url;
    return line ? file + ":" + line + (col ? ":" + col : "") : file;
  }

  // Where a console call came from: the stack's frame for its caller.
  function caller() {
    const frame = (new Error().stack || "").split("\n")[2] || "";
    const at = frame.match(/(\S+?):(\d+):(\d+)\)?$/);
    return at ? sourceOf(at[1].replace(/^.*@/, ""), at[2], at[3]) : undefined;
  }

  for (const level of ["log", "info", "warn", "error", "debug"]) {
    const original = console[level];
    console[level] = function (...args) {
      try {
        keep(level, args.map(describe).join(" "), caller());
      } catch {}
      return original.apply(this, args);
    };
  }

  window.addEventListener(
    "error",
    (e) => {
      if (e instanceof ErrorEvent) {
        keep("error", "Uncaught " + (e.error ? describe(e.error) : e.message), sourceOf(e.filename, e.lineno, e.colno));
      } else if (e.target && e.target !== window) {
        // A resource that did not load: an error that never bubbles,
        // hence the capture phase.
        const el = e.target;
        const url = el.currentSrc || el.src || el.href || "";
        keep("error", "Failed to load <" + el.tagName.toLowerCase() + "> " + url, sourceOf(url));
      }
    },
    true,
  );
  // This webview's label, put in by browser.rs.
  const OWN_WEBVIEW = "__MIRA_WEBVIEW_LABEL__";

  // Not the page's: Tauri plugins inject their scripts into every webview,
  // this one too, and the notification plugin's asks whether it may
  // notify on load — which this webview, rightly, is refused. Exactly
  // that refusal, as Tauri words it in a debug build (naming this
  // webview) and in a release one (naming nothing) — and only once per
  // document, the plugin asking once on load: the same refusal later is
  // the page's own call, and shows. Any other rejection is the page's.
  // Expected only where the plugin asks at all (not on Windows, says
  // browser.rs).
  let ambientCheckPending = __MIRA_AMBIENT_CHECK__;
  function ourPermissionCheckRefused(message) {
    return (
      message === "Command plugin:notification|is_permission_granted not allowed by ACL" ||
      message.startsWith(
        'notification.is_permission_granted not allowed on window "main", webview "' + OWN_WEBVIEW + '"',
      )
    );
  }

  window.addEventListener("unhandledrejection", (e) => {
    const message = describe(e.reason);
    if (ambientCheckPending && ourPermissionCheckRefused(message)) {
      ambientCheckPending = false;
      return;
    }
    keep("error", "Unhandled rejection: " + message);
  });

  function visible(el) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) return false;
    const style = getComputedStyle(el);
    return style.visibility !== "hidden" && style.display !== "none";
  }

  function textOf(el) {
    const t =
      el.getAttribute("aria-label") ||
      el.innerText ||
      el.value ||
      el.alt ||
      el.placeholder ||
      "";
    return String(t).replace(/\s+/g, " ").trim().slice(0, 100);
  }

  function collect() {
    const selector = [
      "a[href]",
      "button",
      "input",
      "select",
      "textarea",
      "[role=button]",
      "[role=link]",
      "[role=checkbox]",
      "[role=tab]",
      "[role=menuitem]",
      "[onclick]",
      "h1",
      "h2",
      "h3",
      "label",
      "img[alt]",
    ].join(",");
    const nodes = [];
    for (const el of document.querySelectorAll(selector)) {
      if (!visible(el)) continue;
      if (!el.dataset.miraId) el.dataset.miraId = String(++nextId);
      const entry = {
        id: Number(el.dataset.miraId),
        tag: el.tagName.toLowerCase(),
        text: textOf(el),
      };
      if (el.type) entry.type = el.type;
      if (el.href) entry.href = el.href;
      if (el.tagName === "INPUT" || el.tagName === "TEXTAREA")
        entry.value = String(el.value).slice(0, 100);
      if (el.tagName === "SELECT") {
        // The choices, so an agent can `fill` one: its text is the chosen
        // option's, not every option's run together.
        entry.text = el.selectedOptions[0] ? optionText(el.selectedOptions[0]) : "";
        entry.value = String(el.value).slice(0, 100);
        entry.options = Array.from(el.options)
          .slice(0, MAX_OPTIONS)
          .map((o) => ({ value: o.value, text: optionText(o), selected: o.selected }));
        if (el.options.length > MAX_OPTIONS) entry.moreOptions = el.options.length - MAX_OPTIONS;
      }
      if (el.checked !== undefined) entry.checked = el.checked;
      if (el.disabled) entry.disabled = true;
      nodes.push(entry);
      if (nodes.length >= 400) break;
    }
    return nodes;
  }

  const MAX_OPTIONS = 50;
  // `text` stops here: a page's worth to read, far under the answer's 1 MB.
  const MAX_TEXT = 100000;

  function optionText(option) {
    return option.text.replace(/\s+/g, " ").trim().slice(0, 100);
  }

  function locate(ref) {
    if (/^\d+$/.test(String(ref)))
      return document.querySelector(`[data-mira-id="${ref}"]`);
    try {
      return document.querySelector(ref);
    } catch {
      return null;
    }
  }

  // Through the prototype's own setter, as a person typing would: React
  // and the like track the value by overriding the element's setter.
  function setNativeValue(el, value) {
    const proto = {
      INPUT: HTMLInputElement.prototype,
      TEXTAREA: HTMLTextAreaElement.prototype,
      SELECT: HTMLSelectElement.prototype,
    }[el.tagName];
    const setter = Object.getOwnPropertyDescriptor(proto, "value").set;
    setter.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }

  // The keys `press` knows, by the name a person would type. Letters and
  // digits are keys too, for chords like Meta+A.
  const KEYS = {
    Enter: "Enter",
    Tab: "Tab",
    Escape: "Escape",
    Backspace: "Backspace",
    Delete: "Delete",
    Space: " ",
    ArrowUp: "ArrowUp",
    ArrowDown: "ArrowDown",
    ArrowLeft: "ArrowLeft",
    ArrowRight: "ArrowRight",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
  };
  const MODIFIERS = { Shift: "shiftKey", Control: "ctrlKey", Alt: "altKey", Meta: "metaKey" };

  // "Shift+Tab" → { key: "Tab", shiftKey: true }; an `error` when unknown.
  function parseKey(name) {
    const parts = String(name).split("+");
    const last = parts.pop();
    const chord = {};
    for (const m of parts) {
      if (!MODIFIERS[m]) return { error: "unknown modifier " + JSON.stringify(m) + "; known: " + Object.keys(MODIFIERS).join(", ") };
      chord[MODIFIERS[m]] = true;
    }
    if (KEYS[last]) chord.key = KEYS[last];
    else if (/^[A-Za-z0-9]$/.test(last)) chord.key = chord.shiftKey ? last.toUpperCase() : last.toLowerCase();
    else
      return {
        error: "unknown key " + JSON.stringify(last) + "; known: " + Object.keys(KEYS).join(", ") + ", a letter or a digit, with " + Object.keys(MODIFIERS).join("+, ") + "+",
      };
    return chord;
  }

  // One key event, as a page sees it. Untrusted (`isTrusted` is false):
  // the page can tell, and a few ignore such events.
  function key(el, type, chord) {
    const code = chord.key.length === 1 ? (/\d/.test(chord.key) ? "Digit" : /[a-z]/i.test(chord.key) ? "Key" : "") + chord.key.toUpperCase() : chord.key;
    return el.dispatchEvent(new KeyboardEvent(type, { ...chord, code, bubbles: true, cancelable: true, composed: true }));
  }

  function isTextField(el) {
    return (el.tagName === "INPUT" && !/^(checkbox|radio|button|submit|reset|file|image|range|color)$/i.test(el.type)) || el.tagName === "TEXTAREA";
  }

  // Edits a text field at its caret, as a key does, and says so with an
  // `input` event — no `change`, which a browser fires on blur.
  function edit(el, text, inputType) {
    if (el.isContentEditable) {
      document.execCommand(inputType === "insertText" ? "insertText" : "delete", false, text);
      return;
    }
    let start = el.value.length;
    let end = start;
    try {
      if (el.selectionStart !== null) [start, end] = [el.selectionStart, el.selectionEnd];
    } catch {} // email and number inputs have no caret to read.
    if (inputType === "deleteContentBackward" && start === end) start = Math.max(0, start - 1);
    if (inputType === "deleteContentForward" && start === end) end = Math.min(el.value.length, end + 1);
    const value = el.value.slice(0, start) + text + el.value.slice(end);
    const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
    try {
      el.setSelectionRange(start + text.length, start + text.length);
    } catch {}
    el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType, data: text || null }));
  }

  function typeInto(el, text) {
    if (!isTextField(el) && !el.isContentEditable)
      return { error: "not a text field: <" + el.tagName.toLowerCase() + "> cannot be typed into (an input, textarea or contenteditable can)" };
    el.focus();
    for (const ch of String(text)) {
      const chord = { key: ch === "\n" ? "Enter" : ch };
      if (key(el, "keydown", chord)) {
        key(el, "keypress", chord);
        edit(el, ch, ch === "\n" ? "insertLineBreak" : "insertText");
      }
      key(el, "keyup", chord);
    }
    return { ok: true };
  }

  const BLOCKS_IMPLICIT_SUBMISSION = /^(text|search|url|tel|email|password|date|month|week|time|datetime-local|number)$/i;

  // HTML's implicit submission (the spec's "Implicit submission", in
  // §4.10.21.2 Form submission): Enter in a field clicks the form's
  // default button — its first submit button — so the button's own
  // handlers and formaction run; a disabled default button means no
  // submission at all. With no submit button, the form submits itself
  // unless more than one of its fields blocks implicit submission.
  function implicitSubmit(form) {
    // Not `form.elements`, which leaves out `<input type=image>` — a
    // submit button all the same. Tree order, the form's own (`form=`
    // included).
    const button = Array.from(document.querySelectorAll("button, input")).find(
      (e) => e.form === form && (e.type === "submit" || e.type === "image"),
    );
    if (button) {
      if (!button.disabled) button.click();
      return;
    }
    const blocking = Array.from(form.elements).filter(
      (e) => e.tagName === "INPUT" && BLOCKS_IMPLICIT_SUBMISSION.test(e.type),
    );
    if (blocking.length <= 1) form.requestSubmit();
  }

  // What tabbing visits, in document order (a positive tabindex's order
  // is not modelled).
  function focusable() {
    return Array.from(
      document.querySelectorAll("a[href], button, input, select, textarea, [tabindex], [contenteditable=true]"),
    ).filter((el) => !el.disabled && el.tabIndex >= 0 && visible(el));
  }

  function press(el, chord) {
    const go = key(el, "keydown", chord);
    const printable = chord.key.length === 1 && !chord.ctrlKey && !chord.metaKey && !chord.altKey;
    if (go && (printable || chord.key === "Enter")) key(el, "keypress", chord);
    if (go) {
      const k = chord.key;
      const command = chord.ctrlKey || chord.metaKey;
      if (k === "Tab") {
        // No wrapping: past the last control (or before the first), focus
        // leaves the page's controls for the document, as in a browser.
        const all = focusable();
        const at = all.indexOf(el);
        const next = at === -1 ? all[chord.shiftKey ? all.length - 1 : 0] : all[at + (chord.shiftKey ? -1 : 1)];
        if (next) next.focus();
        else if (el.blur) el.blur();
      } else if (k === "Enter" && el.tagName === "TEXTAREA") {
        edit(el, "\n", "insertLineBreak");
      } else if (k === "Enter" && isTextField(el) && el.form) {
        // A synthetic Enter submits nothing; a browser's does.
        implicitSubmit(el.form);
      } else if ((k === "Enter" || k === " ") && (el.tagName === "BUTTON" || el.tagName === "A")) {
        el.click();
      } else if ((k === "Backspace" || k === "Delete") && isTextField(el)) {
        edit(el, "", k === "Backspace" ? "deleteContentBackward" : "deleteContentForward");
      } else if (command && k.toLowerCase() === "a" && isTextField(el)) {
        el.select();
      } else if (printable && (isTextField(el) || el.isContentEditable)) {
        edit(el, k, "insertText");
      }
    }
    key(el, "keyup", chord);
    return { ok: true };
  }

  // Fills a field; an answer with `error` when it cannot.
  function fill(el, value) {
    const type = (el.type || "").toLowerCase();
    if (el.tagName === "SELECT") {
      const options = Array.from(el.options);
      const option =
        options.find((o) => o.value === value) ||
        options.find((o) => optionText(o) === String(value).replace(/\s+/g, " ").trim());
      if (!option) {
        const choices = options
          .slice(0, MAX_OPTIONS)
          .map((o) => JSON.stringify(o.value) + " (" + optionText(o) + ")")
          .join(", ");
        return { error: "no option " + JSON.stringify(value) + " in the select; its options: " + choices };
      }
      setNativeValue(el, option.value);
    } else if (el.tagName === "INPUT" && (type === "checkbox" || type === "radio")) {
      return { error: "a " + type + " is not filled: `mira browser click` it to check or uncheck it" };
    } else if (el.tagName === "INPUT" || el.tagName === "TEXTAREA") {
      setNativeValue(el, value);
    } else {
      return { error: "not a field: <" + el.tagName.toLowerCase() + "> takes no value (an input, textarea or select does)" };
    }
    return { ok: true };
  }

  // Answers by id: a Promise's, until Rust takes it with __miraTake.
  const answers = new Map();
  const waiting = new Set();

  function perform(op) {
    if (op.op === "snapshot") {
      return { url: location.href, title: document.title, nodes: collect() };
    } else if (op.op === "click") {
      const el = locate(op.target);
      if (!el) return { error: "element not found: " + op.target };
      el.scrollIntoView({ block: "center", inline: "center" });
      if (el.focus) el.focus();
      el.click();
      return { ok: true };
    } else if (op.op === "fill") {
      const el = locate(op.target);
      if (!el) return { error: "element not found: " + op.target };
      el.focus && el.focus();
      return fill(el, op.value);
    } else if (op.op === "type") {
      const el = locate(op.target);
      if (!el) return { error: "element not found: " + op.target };
      return typeInto(el, op.text);
    } else if (op.op === "press") {
      const chord = parseKey(op.key);
      if (chord.error) return chord;
      let el = document.activeElement || document.body;
      if (op.target !== undefined && op.target !== null) {
        el = locate(op.target);
        if (!el) return { error: "element not found: " + op.target };
        el.focus && el.focus();
      }
      return press(el, chord);
    } else if (op.op === "eval") {
      const value = (0, eval)(op.js);
      if (value && typeof value.then === "function")
        return Promise.resolve(value).then((v) => ({ value: v === undefined ? null : v }));
      // `undefined` has no JSON: without this the key would vanish.
      return { value: value === undefined ? null : value };
    } else if (op.op === "check") {
      return check(op);
    } else if (op.op === "text") {
      const el = op.selector ? document.querySelector(op.selector) : document.body;
      if (!el) return { error: "nothing matches " + op.selector };
      // As a person reads it: innerText, laid out, hidden parts left out.
      const text = el.innerText || "";
      return {
        text:
          text.length > MAX_TEXT ? text.slice(0, MAX_TEXT) + "\n… " + (text.length - MAX_TEXT) + " more characters" : text,
      };
    } else if (op.op === "console") {
      const answer = {
        console: op.errors ? entries.filter((e) => e.level === "error") : entries,
        dropped,
      };
      if (op.clear) {
        entries = [];
        dropped = 0;
      }
      return answer;
    }
    return { error: "unknown op " + op.op };
  }

  // One look for `mira browser wait`, which asks again until it is met.
  // `shows` is a glimpse of the page, for the message when it never is.
  function check(op) {
    const glimpse = () =>
      "the page shows: " + JSON.stringify((document.body ? document.body.innerText : "").replace(/\s+/g, " ").trim().slice(0, 200));
    if (op.shown !== undefined) {
      const met = Array.from(document.querySelectorAll(op.shown)).some(visible);
      return met ? { met } : { met, shows: glimpse() };
    }
    if (op.gone !== undefined) {
      const still = Array.from(document.querySelectorAll(op.gone)).find(visible);
      return still ? { met: false, shows: "still shown: " + JSON.stringify(textOf(still)) } : { met: true };
    }
    if (op.text !== undefined) {
      const met = Boolean(document.body) && document.body.innerText.includes(op.text);
      return met ? { met } : { met, shows: glimpse() };
    }
    return { error: "nothing to check" };
  }

  // Rust refuses a callback string over MAX_ANSWER_BYTES (browser.rs).
  // That string is this JSON encoded again as a JSON string, in UTF-8 —
  // and on macOS with every "/" escaped — so that is what is measured
  // here, to say "too large" before sending rather than be refused.
  const MAX_ANSWER_BYTES = 1000000;

  function encode(result) {
    let json;
    try {
      json = JSON.stringify(result);
    } catch {
      json = JSON.stringify({ error: "result not serializable" });
    }
    const bytes = new TextEncoder().encode(JSON.stringify(json)).length + (json.match(/\//g) || []).length;
    if (bytes > MAX_ANSWER_BYTES)
      return JSON.stringify({ error: "result too large (" + bytes + " bytes, max " + MAX_ANSWER_BYTES + ")" });
    return json;
  }

  // Rust evaluates this and reads back its return value: a JSON string,
  // or `{"pending":true}` for a Promise still running.
  window.__miraRun = function (id, op) {
    let result;
    try {
      result = perform(op);
    } catch (e) {
      result = { error: String(e) };
    }
    if (!(result instanceof Promise)) return encode(result);
    waiting.add(id);
    result
      .catch((e) => ({ error: String(e) }))
      .then((r) => {
        waiting.delete(id);
        answers.set(id, encode(r));
      });
    return JSON.stringify({ pending: true });
  };

  window.__miraTake = function (id) {
    if (answers.has(id)) {
      const answer = answers.get(id);
      answers.delete(id);
      return answer;
    }
    if (waiting.has(id)) return JSON.stringify({ pending: true });
    return JSON.stringify({ error: "the page changed before it answered" });
  };
})();
