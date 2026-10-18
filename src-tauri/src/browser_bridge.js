// Injected into every browser-pane webview. Exposes window.__miraRun for
// the automation bridge. Rust evaluates it and reads its return value
// (`eval_with_callback`) — the page never gets IPC access to the app, and
// nothing navigates, so a click's own navigation is left alone.
(function () {
  if (window.__miraInit) return;
  window.__miraInit = true;
  let nextId = 0;

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
    } else if (op.op === "eval") {
      const value = (0, eval)(op.js);
      if (value && typeof value.then === "function")
        return Promise.resolve(value).then((v) => ({ value: v === undefined ? null : v }));
      // `undefined` has no JSON: without this the key would vanish.
      return { value: value === undefined ? null : value };
    } else if (op.op === "check") {
      return check(op);
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
