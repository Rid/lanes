// Lanes manager window: add, adopt, rename, recolour and remove lanes.
// Launching lanes happens in the native menu-bar menu (src-tauri/src/menu.rs).
// No localStorage/sessionStorage: all state lives in Rust (JSON on disk).

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Auto-assign palette, starting from Appbox violet. Each colour fills an app tile
// behind a white monogram, so all of them are dark enough for white text.
const LANE_COLORS = ["#593ee8", "#3023ae", "#8a3ec8", "#2563eb", "#0b7a70", "#c2366b"];

// Phosphor icons (MIT, regular weight), the set Kumo uses.
const ICON_PATHS = {
  plus: "M224,128a8,8,0,0,1-8,8H136v80a8,8,0,0,1-16,0V136H40a8,8,0,0,1,0-16h80V40a8,8,0,0,1,16,0v80h80A8,8,0,0,1,224,128Z",
  trash:
    "M216,48H176V40a24,24,0,0,0-24-24H104A24,24,0,0,0,80,40v8H40a8,8,0,0,0,0,16h8V208a16,16,0,0,0,16,16H192a16,16,0,0,0,16-16V64h8a8,8,0,0,0,0-16ZM96,40a8,8,0,0,1,8-8h48a8,8,0,0,1,8,8v8H96Zm96,168H64V64H192ZM112,104v64a8,8,0,0,1-16,0V104a8,8,0,0,1,16,0Zm48,0v64a8,8,0,0,1-16,0V104a8,8,0,0,1,16,0Z",
  lock: "M208,80H176V56a48,48,0,0,0-96,0V80H48A16,16,0,0,0,32,96V208a16,16,0,0,0,16,16H208a16,16,0,0,0,16-16V96A16,16,0,0,0,208,80ZM96,56a32,32,0,0,1,64,0V80H96ZM208,208H48V96H208V208Z",
  signIn:
    "M141.66,133.66l-40,40a8,8,0,0,1-11.32-11.32L116.69,136H24a8,8,0,0,1,0-16h92.69L90.34,93.66a8,8,0,0,1,11.32-11.32l40,40A8,8,0,0,1,141.66,133.66ZM200,32H136a8,8,0,0,0,0,16h56V208H136a8,8,0,0,0,0,16h64a8,8,0,0,0,8-8V40A8,8,0,0,0,200,32Z",
  folder:
    "M216,72H130.67L102.93,51.2a16.12,16.12,0,0,0-9.6-3.2H40A16,16,0,0,0,24,64V200a16,16,0,0,0,16,16H216.89A15.13,15.13,0,0,0,232,200.89V88A16,16,0,0,0,216,72Zm0,128H40V64H93.33L123.2,86.4A8,8,0,0,0,128,88h88Z",
  plug: "M149.66,138.34a8,8,0,0,0-11.32,0L120,156.69,99.31,136l18.35-18.34a8,8,0,0,0-11.32-11.32L88,124.69,69.66,106.34a8,8,0,0,0-11.32,11.32L64.69,124,41.37,147.31a32,32,0,0,0,0,45.26l5.38,5.37-28.41,28.4a8,8,0,0,0,11.32,11.32l28.4-28.41,5.37,5.38a32,32,0,0,0,45.26,0L132,191.31l6.34,6.35a8,8,0,0,0,11.32-11.32L131.31,168l18.35-18.34A8,8,0,0,0,149.66,138.34Zm-52.29,65a16,16,0,0,1-22.62,0L52.69,181.25a16,16,0,0,1,0-22.62L76,135.31,120.69,180Zm140.29-185a8,8,0,0,0-11.32,0l-28.4,28.41-5.37-5.38a32.05,32.05,0,0,0-45.26,0L124,64.69l-6.34-6.35a8,8,0,0,0-11.32,11.32l80,80a8,8,0,0,0,11.32-11.32L191.31,132l23.32-23.31a32,32,0,0,0,0-45.26l-5.38-5.37,28.41-28.4A8,8,0,0,0,237.66,18.34Zm-34.35,79L180,120.69,135.31,76l23.32-23.31a16,16,0,0,1,22.62,0l22.06,22A16,16,0,0,1,203.31,97.37Z",
  warning:
    "M128,24A104,104,0,1,0,232,128,104.11,104.11,0,0,0,128,24Zm0,192a88,88,0,1,1,88-88A88.1,88.1,0,0,1,128,216Zm-8-80V80a8,8,0,0,1,16,0v56a8,8,0,0,1-16,0Zm20,36a12,12,0,1,1-12-12A12,12,0,0,1,140,172Z",
};

// A line of text with an icon aligned to its first line (Kumo's icon rule).
function iconLine(cls, iconName, text) {
  const row = el("div", cls);
  const slot = el("span", "line-icon");
  slot.appendChild(icon(iconName));
  row.append(slot, el("span", null, text));
  return row;
}

const state = {
  apps: [],
  appByKey: {},
  profiles: [],
  adoptable: [],
  // "manage" | "add" | "adopt" | "confirm" | "copy" | "copied"
  mode: "manage",
  copy: { from: null, to: null, busy: false, result: null },
  // Where a form returns to: "manage", or "close" when the menu opened it directly.
  back: "close",
  adoptKey: null,
  confirmId: null,
  colorEditId: null,
  draft: { app: null, label: "", color: LANE_COLORS[0] },
  error: "",
};

const $body = () => document.getElementById("body");
const $foot = () => document.getElementById("foot");

// ---- data ----
async function load() {
  const [apps, profiles, adoptable] = await Promise.all([
    invoke("known_apps"),
    invoke("list_profiles"),
    invoke("adoptable_apps"),
  ]);
  state.apps = apps;
  state.appByKey = Object.fromEntries(apps.map((a) => [a.key, a]));
  state.profiles = profiles;
  state.adoptable = adoptable;
}

async function refresh() {
  await load();
  render();
}

// ---- navigation ----

// A view requested by the menu ("manage", "add" or "adopt:<app>").
async function openView(view) {
  await load();
  state.error = "";
  state.colorEditId = null;
  if (view === "add") {
    startAdd("close");
  } else if (view === "copy") {
    startCopy("close");
  } else if (view.startsWith("adopt:")) {
    state.adoptKey = view.slice("adopt:".length);
    state.back = "close";
    state.mode = "adopt";
  } else {
    state.mode = "manage";
  }
  render();
}

// Leave a form: back to the list, or close the window if the menu opened the form.
function leaveForm() {
  state.error = "";
  if (state.back === "close") return closeWindow();
  state.mode = "manage";
  render();
}

function closeWindow() {
  invoke("close_manager");
}

// ---- helpers ----
function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

function icon(name) {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("class", "icon");
  svg.setAttribute("viewBox", "0 0 256 256");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(NS, "path");
  path.setAttribute("d", ICON_PATHS[name]);
  svg.appendChild(path);
  return svg;
}

function tile(app, color, tag = "span") {
  const t = el(tag, "ic", app ? app.monogram : "?");
  t.style.background = color;
  return t;
}

function appName(p) {
  const app = state.appByKey[p.app];
  return app ? app.name : p.app;
}

function nextColor() {
  const used = new Set(state.profiles.map((p) => p.color));
  return LANE_COLORS.find((c) => !used.has(c)) || LANE_COLORS[state.profiles.length % LANE_COLORS.length];
}

function title(text, sub) {
  const wrap = el("div", "view-head");
  wrap.appendChild(el("h1", "view-title", text));
  if (sub) wrap.appendChild(el("p", "view-sub", sub));
  return wrap;
}

// Errors show as a Kumo danger banner.
function errorLine() {
  if (!state.error) return null;
  const banner = iconLine("err", "warning", state.error);
  banner.setAttribute("role", "alert");
  return banner;
}

function swatches(selected, onPick) {
  const sw = el("div", "swatches");
  LANE_COLORS.forEach((c) => {
    const s = el("button", "swatch" + (selected === c ? " sel" : ""));
    s.type = "button";
    s.style.background = c;
    s.setAttribute("aria-label", `Colour ${c}`);
    s.addEventListener("click", () => onPick(c));
    sw.appendChild(s);
  });
  return sw;
}

// ---- render ----
function render() {
  const body = $body();
  body.innerHTML = "";
  $foot().hidden = state.mode !== "manage";
  document.getElementById("copyBtn").hidden = claudeLanes().length < 2;

  if (state.mode === "add") renderAdd(body);
  else if (state.mode === "copy") renderCopy(body);
  else if (state.mode === "copied") renderCopied(body);
  else if (state.mode === "adopt") renderAdopt(body);
  else if (state.mode === "confirm") renderConfirm(body);
  else renderManage(body);
  afterRender();
}

function renderManage(body) {
  body.appendChild(title("Lanes", "Each lane is a separate sign-in. Open them from the menu bar."));

  state.adoptable.forEach((a) => body.appendChild(adoptCard(a)));
  const err = errorLine();
  if (err) body.appendChild(err);

  if (state.profiles.length === 0) {
    body.appendChild(el("p", "empty-hint", "No lanes yet. Add one for each account — Work, Personal, whatever fits."));
    return;
  }
  const list = el("div", "list");
  state.profiles.forEach((p) => {
    list.appendChild(manageRow(p));
    if (state.colorEditId === p.id) {
      const picker = el("div", "row-colors");
      picker.appendChild(
        swatches(p.color, async (c) => {
          state.colorEditId = null;
          await invoke("set_profile_color", { id: p.id, color: c }).catch((e) => (state.error = String(e)));
          refresh();
        }),
      );
      list.appendChild(picker);
    }
  });
  body.appendChild(list);
}

// "You're already signed into Claude — add it as a lane."
function adoptCard(a, { focus = false } = {}) {
  const wrap = el("div", "adopt");

  const hd = el("div", "adopt-hd");
  hd.appendChild(tile(a, a.default_color));
  const t = el("div", "adopt-t");
  t.appendChild(el("span", "adopt-name", `${a.name} — already signed in`));
  t.appendChild(el("span", "adopt-sub", a.running ? "Running now · name your current session" : "Add your current session as a lane"));
  hd.appendChild(t);
  if (a.running) hd.appendChild(el("span", "live"));
  wrap.appendChild(hd);

  const row = el("div", "adopt-row");
  const input = el("input", "text-input adopt-input");
  input.placeholder = "Name it — e.g. Personal";
  input.maxLength = 32;
  const btn = el("button", "btn btn-primary adopt-btn", "Add");
  btn.type = "button";
  btn.disabled = true;
  const submit = async () => {
    const v = input.value.trim();
    if (!v) return;
    try {
      state.error = "";
      await invoke("adopt_profile", { app: a.key, label: v });
      if (state.mode === "adopt" && state.back === "close") return closeWindow();
      state.mode = "manage";
      await refresh();
    } catch (e) {
      state.error = String(e);
      render();
    }
  };
  input.addEventListener("input", () => (btn.disabled = input.value.trim().length === 0));
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && input.value.trim()) submit();
  });
  btn.addEventListener("click", submit);
  row.append(input, btn);
  wrap.appendChild(row);
  if (focus) setTimeout(() => input.focus(), 30);
  return wrap;
}

function manageRow(p) {
  const app = state.appByKey[p.app];
  const row = el("div", "lane");

  const colorBtn = tile(app, p.color, "button");
  colorBtn.type = "button";
  colorBtn.title = "Change colour";
  colorBtn.setAttribute("aria-label", `Change colour of ${p.label}`);
  colorBtn.addEventListener("click", () => {
    state.colorEditId = state.colorEditId === p.id ? null : p.id;
    render();
  });

  const meta = el("div", "meta");
  const input = el("input", "rename-input");
  input.value = p.label;
  input.maxLength = 32;
  input.setAttribute("aria-label", `Rename ${appName(p)} lane`);
  const save = () => {
    const v = input.value.trim();
    if (!v) {
      input.value = p.label; // an emptied name reverts instead of sitting blank
      return;
    }
    if (v !== p.label) {
      invoke("rename_profile", { id: p.id, label: v })
        .then(refresh)
        .catch(() => (input.value = p.label));
    }
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") input.blur();
    if (e.key === "Escape") {
      e.stopPropagation(); // cancel the edit only; don't also close the window
      input.value = p.label;
      input.blur();
    }
  });
  input.addEventListener("blur", save);
  const sub = el("span", "prof", p.isDefault ? `${appName(p)} · current session` : appName(p));
  meta.append(input, sub);

  const del = el("button", "icon-btn danger");
  del.type = "button";
  del.appendChild(icon("trash"));
  del.title = "Remove lane";
  del.setAttribute("aria-label", `Remove ${p.label}`);
  del.addEventListener("click", () => {
    state.error = "";
    state.confirmId = p.id;
    state.back = "manage";
    state.mode = "confirm";
    render();
  });

  row.append(colorBtn, meta, del);
  return row;
}

function renderAdopt(body) {
  const a = state.adoptable.find((x) => x.key === state.adoptKey);
  if (!a) {
    // Already added (or the session went away) since the menu was opened.
    body.appendChild(title("Nothing to add", "That session is already a lane."));
    body.appendChild(formActions(null, "Close"));
    return;
  }
  body.appendChild(
    title(`Add your current ${a.name} session`, "The account you're signed into now becomes a lane. You won't need to sign in again."),
  );
  body.appendChild(adoptCard(a, { focus: true }));
  const err = errorLine();
  if (err) body.appendChild(err);
  body.appendChild(formActions(null, "Cancel"));
}

function startAdd(back) {
  const firstInstalled = state.apps.find((a) => a.installed);
  state.draft = {
    app: firstInstalled ? firstInstalled.key : null,
    label: "",
    color: nextColor(),
  };
  state.error = "";
  state.back = back;
  state.mode = "add";
}

function renderAdd(body) {
  body.appendChild(title("Add a lane", "Name it, then sign in once in the window that opens."));
  const form = el("div", "form");

  form.appendChild(el("p", "f-label", "App"));
  const picks = el("div", "app-picks");
  state.apps.forEach((a) => {
    const pick = el("button", "app-pick" + (a.installed ? "" : " disabled") + (state.draft.app === a.key ? " sel" : ""));
    pick.type = "button";
    pick.disabled = !a.installed;
    pick.appendChild(tile(a, a.default_color));
    pick.appendChild(el("span", "an", a.name));
    if (!a.installed) pick.appendChild(el("span", "ni", "not installed"));
    pick.addEventListener("click", () => {
      state.draft.app = a.key;
      render();
    });
    picks.appendChild(pick);
  });
  form.appendChild(picks);

  form.appendChild(el("p", "f-label", "Name"));
  const input = el("input", "text-input");
  input.placeholder = "Work";
  input.value = state.draft.label;
  input.maxLength = 32;
  form.appendChild(input);

  form.appendChild(el("p", "f-label", "Colour"));
  form.appendChild(
    swatches(state.draft.color, (c) => {
      state.draft.color = c;
      render();
    }),
  );

  const err = errorLine();
  if (err) form.appendChild(err);

  const addBtn = el("button", "btn btn-primary", "Add lane");
  addBtn.type = "button";
  addBtn.disabled = !canAdd();
  addBtn.addEventListener("click", submitAdd);
  input.addEventListener("input", () => {
    state.draft.label = input.value;
    addBtn.disabled = !canAdd();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && canAdd()) submitAdd();
  });
  form.appendChild(formActions(addBtn, "Cancel"));

  body.appendChild(form);
  setTimeout(() => input.focus(), 30);
}

function formActions(primary, cancelText) {
  const actions = el("div", "form-actions");
  if (primary) actions.appendChild(primary);
  const cancel = el("button", "btn btn-ghost", cancelText);
  cancel.type = "button";
  cancel.addEventListener("click", leaveForm);
  actions.appendChild(cancel);
  return actions;
}

function renderConfirm(body) {
  const p = state.profiles.find((x) => x.id === state.confirmId);
  if (!p) {
    state.mode = "manage";
    return renderManage(body);
  }
  body.appendChild(title("Remove this lane?", `“${p.label}” (${appName(p)}) will be removed from Lanes.`));
  const wrap = el("div", "confirm");

  // Adopted default sessions: never offer to delete the app's real data folder.
  let cb = null;
  if (p.isDefault) {
    wrap.appendChild(
      el(
        "div",
        "c-note",
        `This is your existing ${appName(p)} session. Removing it only takes the lane out of Lanes; your ${appName(p)} data and sign-in stay where they are.`,
      ),
    );
  } else {
    const label = el("label", "check");
    cb = el("input");
    cb.type = "checkbox";
    label.appendChild(cb);
    const txt = el("div");
    txt.appendChild(document.createTextNode("Also delete its data folder"));
    txt.appendChild(el("span", "sub", "Erases the saved sign-in for this lane. Can’t be undone."));
    label.appendChild(txt);
    wrap.appendChild(label);

    const path = el("div", "c-path", p.dataDir);
    path.hidden = true;
    cb.addEventListener("change", () => {
      path.hidden = !cb.checked;
      afterRender();
    });
    wrap.appendChild(path);
  }

  const err = errorLine();
  if (err) wrap.appendChild(err);

  const remove = el("button", "btn btn-danger", "Remove");
  remove.type = "button";
  remove.addEventListener("click", async () => {
    try {
      state.error = "";
      await invoke("remove_profile", { id: p.id, deleteData: cb ? cb.checked : false });
      state.confirmId = null;
      state.mode = "manage";
      await refresh();
    } catch (e) {
      state.error = String(e);
      render();
    }
  });
  wrap.appendChild(formActions(remove, "Cancel"));
  body.appendChild(wrap);
}

// ---- copy setup between lanes ----
const claudeLanes = () => state.profiles.filter((p) => p.app === "claude");

function startCopy(back) {
  const lanes = claudeLanes();
  // From the session you already use (the adopted one) to the newest lane.
  const from = lanes.find((p) => p.isDefault) || lanes[0];
  const to = [...lanes].reverse().find((p) => p !== from);
  state.copy = { from: from?.id ?? null, to: to?.id ?? null, busy: false, result: null };
  state.error = "";
  state.back = back;
  state.mode = "copy";
}

function laneSelect(value, onChange) {
  const select = el("select", "select");
  claudeLanes().forEach((p) => {
    const opt = el("option", null, p.label);
    opt.value = p.id;
    opt.selected = p.id === value;
    select.appendChild(opt);
  });
  select.addEventListener("change", () => onChange(select.value));
  return select;
}

function renderCopy(body) {
  body.appendChild(title("Copy Claude setup", "Make one lane match another: settings, plugins, and Code and Cowork sessions."));
  const form = el("div", "form");
  if (claudeLanes().length < 2) {
    form.appendChild(el("p", "c-note", "Add a second Claude lane first, then copy your setup into it."));
    form.appendChild(formActions(null, "Close"));
    body.appendChild(form);
    return;
  }
  const c = state.copy;
  form.appendChild(el("p", "f-label", "From"));
  form.appendChild(laneSelect(c.from, (v) => ((c.from = v), render())));
  form.appendChild(el("p", "f-label", "To"));
  form.appendChild(laneSelect(c.to, (v) => ((c.to = v), render())));

  const label = (id) => state.profiles.find((p) => p.id === id)?.label ?? "that lane";
  const notes = el("div", "notes");
  notes.append(
    iconLine("note", "lock", "Sign-in isn't copied. Chats and projects stay with each account on claude.ai."),
    iconLine("note", "signIn", `Sign in to “${label(c.to)}” once first, and quit it before copying.`),
    iconLine("note", "folder", `Copied sessions share their working folders with “${label(c.from)}”.`),
  );
  form.appendChild(notes);

  const err = errorLine();
  if (err) form.appendChild(err);

  const go = el("button", "btn btn-primary", c.busy ? "Copying…" : "Copy");
  go.type = "button";
  go.disabled = c.busy || !c.from || !c.to || c.from === c.to;
  go.addEventListener("click", submitCopy);
  const actions = formActions(go, "Cancel");
  actions.lastChild.disabled = c.busy;
  form.appendChild(actions);
  body.appendChild(form);
}

async function submitCopy() {
  const c = state.copy;
  c.busy = true;
  state.error = "";
  render();
  try {
    c.result = await invoke("copy_lane", { from: c.from, to: c.to });
    state.mode = "copied";
  } catch (e) {
    state.error = String(e);
  }
  c.busy = false;
  render();
}

function renderCopied(body) {
  const toLabel = state.profiles.find((p) => p.id === state.copy.to)?.label ?? "the other lane";
  const sessions = (n) => (n === 1 ? "1 session" : `${n} sessions`);
  const { sessions: copied = 0, kept_newer: kept = 0, connect = [] } = state.copy.result ?? {};
  body.appendChild(
    title("Setup copied", `${sessions(copied)} and your settings are now in “${toLabel}”. Open it from the menu to see them.`),
  );
  if (kept > 0) {
    const notes = el("div", "notes copied-notes");
    notes.appendChild(
      iconLine("note", "folder", `${sessions(kept)} you'd used more recently in “${toLabel}” ${kept === 1 ? "was" : "were"} kept as ${kept === 1 ? "it was" : "they were"}.`),
    );
    body.appendChild(notes);
  }
  // claude.ai connectors belong to each account, so the copy can't bring them.
  if (connect.length > 0) {
    const section = el("div", "connect");
    section.appendChild(el("h2", "connect-title", `Connect these in “${toLabel}”`));
    section.appendChild(
      el("p", "connect-sub", `They belong to each claude.ai account, so they can't be copied. In ${toLabel}, open Settings → Connectors.`),
    );
    const list = el("div", "notes");
    connect.forEach((c) => list.appendChild(iconLine("note connect-item", "plug", c.name)));
    section.appendChild(list);
    body.appendChild(section);
  }
  const done = el("button", "btn btn-primary", "Done");
  done.type = "button";
  done.addEventListener("click", leaveForm);
  const actions = el("div", "form-actions");
  actions.appendChild(done);
  body.appendChild(actions);
}

// ---- actions ----
function canAdd() {
  const a = state.appByKey[state.draft.app];
  return !!a && a.installed && state.draft.label.trim().length > 0;
}

async function submitAdd() {
  if (!canAdd()) return;
  try {
    state.error = "";
    await invoke("add_profile", {
      app: state.draft.app,
      label: state.draft.label.trim(),
      color: state.draft.color,
    });
    if (state.back === "close") return closeWindow();
    state.mode = "manage";
    await refresh();
  } catch (e) {
    state.error = String(e);
    render();
  }
}

// ---- window height follows the content ----
function afterRender() {
  requestAnimationFrame(() => {
    const h = document.getElementById("panel").offsetHeight;
    invoke("fit_manager", { height: h });
  });
}

// ---- keyboard ----
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || state.copy.busy) return;
  if (state.mode === "manage") return closeWindow();
  leaveForm();
});

// ---- footer wiring ----
document.getElementById("addBtn").addEventListener("click", () => {
  startAdd("manage");
  render();
});
document.getElementById("copyBtn").addEventListener("click", () => {
  startCopy("manage");
  render();
});
document.getElementById("doneBtn").addEventListener("click", closeWindow);

// Adoptable sessions change as apps start and quit; recheck when the window comes back.
window.addEventListener("focus", () => {
  if (state.mode === "manage") refresh();
});

// The menu asks for a view while the window is already open.
listen("lanes://view", (e) => openView(String(e.payload)));

// ---- boot ----
openView(window.__LANES_VIEW || "manage");
