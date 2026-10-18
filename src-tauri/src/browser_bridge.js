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
      if (el.checked !== undefined) entry.checked = el.checked;
      if (el.disabled) entry.disabled = true;
      nodes.push(entry);
      if (nodes.length >= 400) break;
    }
    return nodes;
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

  function setNativeValue(el, value) {
    const proto =
      el.tagName === "TEXTAREA"
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value").set;
    setter.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
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
      setNativeValue(el, op.value);
      return { ok: true };
    } else if (op.op === "eval") {
      const value = (0, eval)(op.js);
      if (value && typeof value.then === "function")
        return Promise.resolve(value).then((v) => ({ value: v === undefined ? null : v }));
      // `undefined` has no JSON: without this the key would vanish.
      return { value: value === undefined ? null : value };
    }
    return { error: "unknown op " + op.op };
  }

  function encode(result) {
    let json;
    try {
      json = JSON.stringify(result);
    } catch {
      json = JSON.stringify({ error: "result not serializable" });
    }
    return json.length > 1000000 ? JSON.stringify({ error: "result too large" }) : json;
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
