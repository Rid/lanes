// Lanes tray panel — renders lanes, drives add/rename/remove, talks to Rust.
// No localStorage/sessionStorage: all state lives in Rust (JSON on disk).

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);

// Auto-assign palette (teal → soft → blue → variants), matching the brand.
const LANE_COLORS = ["#008080", "#16a3a3", "#4374ba", "#6a8fd0", "#0e8f8f", "#2e5e9e"];

const state = {
  apps: [],
  appByKey: {},
  profiles: [],
  adoptable: [],
  running: new Set(),
  mode: "list", // list | add | manage | confirm
  selected: 0,
  confirmId: null,
  draft: { app: null, label: "", color: LANE_COLORS[0] },
  error: "",
};

const $body = () => document.getElementById("body");
const $foot = () => document.getElementById("foot");
const $count = () => document.getElementById("count");

// ---- data ----
async function refresh() {
  const [apps, profiles, running, adoptable] = await Promise.all([
    invoke("known_apps"),
    invoke("list_profiles"),
    invoke("running_dirs"),
    invoke("adoptable_apps"),
  ]);
  state.apps = apps;
  state.appByKey = Object.fromEntries(apps.map((a) => [a.key, a]));
  state.profiles = profiles;
  state.adoptable = adoptable;
  state.running = new Set(running);
  if (state.selected >= profiles.length) state.selected = Math.max(0, profiles.length - 1);
  render();
}

function refreshRunning() {
  // Re-pull running + adoptable together (a newly-started default session becomes
  // both "running" and "adoptable").
  Promise.all([invoke("running_dirs"), invoke("adoptable_apps")]).then(([running, adoptable]) => {
    state.running = new Set(running);
    state.adoptable = adoptable;
    if (state.mode === "list") render();
  });
}

// ---- helpers ----
function laneIcon(app) {
  const ic = el("span", "ic", app ? app.monogram : "?");
  return ic;
}

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

function nextColor() {
  const used = new Set(state.profiles.map((p) => p.color));
  return LANE_COLORS.find((c) => !used.has(c)) || LANE_COLORS[state.profiles.length % LANE_COLORS.length];
}

// ---- render ----
function render() {
  const body = $body();
  body.innerHTML = "";
  $count().hidden = true;

  if (state.mode === "add") return renderAdd(body);
  if (state.mode === "confirm") return renderConfirm(body);

  // list or manage
  $foot().hidden = false;
  updateFooter();

  // "You're already signed into Claude — adopt it" strips (list mode only).
  if (state.mode === "list") {
    state.adoptable.forEach((a) => body.appendChild(adoptStrip(a)));
  }

  if (state.error && state.mode === "list") {
    const bar = el("div", "err");
    bar.style.padding = "10px 15px 0";
    bar.textContent = state.error;
    body.appendChild(bar);
  }

  if (state.profiles.length === 0) {
    // Full empty state only when there's nothing to adopt either; otherwise the
    // adopt strip is the primary action and the footer "+ Add profile" covers new ones.
    if (state.adoptable.length === 0) renderEmpty(body);
    else body.appendChild(emptyHint());
    afterRender();
    return;
  }

  // running count chip
  const liveCount = state.profiles.filter((p) => state.running.has(p.dataDir)).length;
  if (liveCount > 0) {
    $count().hidden = false;
    $count().textContent = `${liveCount} open`;
  }

  state.profiles.forEach((p, i) => {
    state.mode === "manage" ? body.appendChild(manageRow(p)) : body.appendChild(laneRow(p, i));
  });
  afterRender();
}

function adoptStrip(a) {
  const wrap = el("div", "adopt");

  const hd = el("div", "adopt-hd");
  const ic = el("span", "ic", a.monogram);
  ic.style.borderColor = a.default_color;
  hd.appendChild(ic);
  const t = el("div", "adopt-t");
  t.appendChild(el("span", "adopt-name", `${a.name} — already signed in`));
  t.appendChild(el("span", "adopt-sub", a.running ? "Running now · name your current session" : "Add your current session as a lane"));
  hd.appendChild(t);
  if (a.running) hd.appendChild(el("span", "live"));
  wrap.appendChild(hd);

  const rowc = el("div", "adopt-row");
  const input = el("input", "text-input adopt-input");
  input.placeholder = "Name it — e.g. Personal";
  input.maxLength = 32;
  const btn = el("button", "btn btn-primary adopt-btn", "Add");
  btn.disabled = true;
  const submit = async () => {
    const v = input.value.trim();
    if (!v) return;
    try {
      state.error = "";
      await invoke("adopt_profile", { app: a.key, label: v });
      state.mode = "list";
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
  rowc.append(input, btn);
  wrap.appendChild(rowc);
  return wrap;
}

function emptyHint() {
  const h = el("div", "empty-hint", "…or use + Add profile below for a separate account.");
  return h;
}

function laneRow(p, i) {
  const app = state.appByKey[p.app];
  const row = el("div", "lane launchable");
  row.setAttribute("role", "listitem");
  row.tabIndex = 0;
  if (i === state.selected) row.classList.add("sel");

  const dot = el("span", "dotlane");
  dot.style.background = p.color;

  const ic = laneIcon(app);
  ic.style.borderColor = p.color;

  const meta = el("div", "meta");
  const appRow = el("div", "app-row");
  appRow.appendChild(el("span", "app", app ? app.name : p.app));
  if (p.isDefault) appRow.appendChild(el("span", "badge", "current"));
  meta.appendChild(appRow);
  meta.appendChild(el("span", "prof", p.label));

  row.append(dot, ic, meta, el("span", "spacer"));
  if (state.running.has(p.dataDir)) row.appendChild(el("span", "live"));
  row.appendChild(el("span", "go", "›"));

  const launch = () => launchProfile(p);
  row.addEventListener("click", launch);
  row.addEventListener("mouseenter", () => {
    state.selected = i;
    [...$body().querySelectorAll(".lane")].forEach((r, j) => r.classList.toggle("sel", j === i));
  });
  return row;
}

function manageRow(p) {
  const app = state.appByKey[p.app];
  const row = el("div", "lane");

  const dot = el("span", "dotlane");
  dot.style.background = p.color;
  const ic = laneIcon(app);
  ic.style.borderColor = p.color;

  const input = el("input", "rename-input");
  input.value = p.label;
  input.setAttribute("aria-label", `Rename ${app ? app.name : p.app} profile`);
  const save = () => {
    const v = input.value.trim();
    if (v && v !== p.label) invoke("rename_profile", { id: p.id, label: v }).then(refresh);
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") input.blur();
    if (e.key === "Escape") {
      input.value = p.label;
      input.blur();
    }
  });
  input.addEventListener("blur", save);

  const actions = el("div", "row-actions");
  const del = el("button", "icon-btn danger", "🗑");
  del.title = "Remove profile";
  del.setAttribute("aria-label", `Remove ${app ? app.name : p.app} · ${p.label}`);
  del.addEventListener("click", () => {
    state.confirmId = p.id;
    state.mode = "confirm";
    render();
  });
  actions.appendChild(del);

  row.append(dot, ic, input, actions);
  return row;
}

function renderEmpty(body) {
  const wrap = el("div", "empty");
  wrap.appendChild(el("div", "e-title", "No lanes yet"));
  const sub = el(
    "div",
    "e-sub",
    "Add a lane for each account — name them Work, Personal, whatever fits. Sign in once per lane.",
  );
  wrap.appendChild(sub);
  const btn = el("button", "btn btn-primary", "+ Add your first lane");
  btn.style.maxWidth = "220px";
  btn.style.margin = "0 auto";
  btn.style.display = "block";
  btn.addEventListener("click", openAdd);
  wrap.appendChild(btn);
  body.appendChild(wrap);
}

function renderAdd(body) {
  $foot().hidden = true;
  const form = el("div", "form");

  form.appendChild(el("p", "f-label", "App"));
  const picks = el("div", "app-picks");
  state.apps.forEach((a) => {
    const pick = el("div", "app-pick" + (a.installed ? "" : " disabled") + (state.draft.app === a.key ? " sel" : ""));
    const ic = el("span", "ic", a.monogram);
    ic.style.borderColor = a.installed ? a.default_color : "var(--line)";
    pick.appendChild(ic);
    pick.appendChild(el("span", "an", a.name));
    if (!a.installed) pick.appendChild(el("span", "ni", "not installed"));
    if (a.installed) {
      pick.addEventListener("click", () => {
        state.draft.app = a.key;
        render();
      });
    }
    picks.appendChild(pick);
  });
  form.appendChild(picks);

  form.appendChild(el("p", "f-label", "Profile name"));
  const input = el("input", "text-input");
  input.placeholder = "Work";
  input.value = state.draft.label;
  input.maxLength = 32;
  input.addEventListener("input", () => {
    state.draft.label = input.value;
    addBtn.disabled = !canAdd();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && canAdd()) submitAdd();
    if (e.key === "Escape") closeForm();
  });
  form.appendChild(input);

  form.appendChild(el("p", "f-label", "Lane color"));
  const sw = el("div", "swatches");
  LANE_COLORS.forEach((c) => {
    const s = el("button", "swatch" + (state.draft.color === c ? " sel" : ""));
    s.style.background = c;
    s.setAttribute("aria-label", `Color ${c}`);
    s.addEventListener("click", () => {
      state.draft.color = c;
      render();
    });
    sw.appendChild(s);
  });
  form.appendChild(sw);

  const err = el("div", "err", state.error || "");
  form.appendChild(err);

  const actions = el("div", "form-actions");
  const addBtn = el("button", "btn btn-primary", "Add lane");
  addBtn.disabled = !canAdd();
  addBtn.addEventListener("click", submitAdd);
  const cancel = el("button", "btn btn-ghost", "Cancel");
  cancel.addEventListener("click", closeForm);
  actions.append(addBtn, cancel);
  form.appendChild(actions);

  body.appendChild(form);
  afterRender();
  setTimeout(() => input.focus(), 30);
}

function renderConfirm(body) {
  $foot().hidden = true;
  const p = state.profiles.find((x) => x.id === state.confirmId);
  if (!p) {
    state.mode = "manage";
    return render();
  }
  const app = state.appByKey[p.app];

  const wrap = el("div", "confirm");
  wrap.appendChild(el("div", "c-title", "Remove this profile?"));
  wrap.appendChild(el("div", "c-text", `“${app ? app.name : p.app} · ${p.label}” will be removed from Lanes.`));

  // Adopted default sessions: never offer to delete the app's real data folder.
  let cb = null;
  if (p.isDefault) {
    wrap.appendChild(
      el(
        "div",
        "c-note",
        "This is your existing default session. Removing it just takes the lane out of Lanes — your Claude data and login stay exactly where they are.",
      ),
    );
  } else {
    const label = el("label", "check");
    cb = el("input");
    cb.type = "checkbox";
    label.appendChild(cb);
    const txt = el("div");
    txt.appendChild(document.createTextNode("Also delete its data folder"));
    txt.appendChild(el("span", "sub", "Erases the saved login for this lane. Can’t be undone."));
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

  if (state.error) wrap.appendChild(el("div", "err", state.error));

  const actions = el("div", "form-actions");
  const remove = el("button", "btn btn-danger", "Remove");
  remove.addEventListener("click", async () => {
    try {
      state.error = "";
      await invoke("remove_profile", { id: p.id, deleteData: cb ? cb.checked : false });
      state.confirmId = null;
      state.mode = state.profiles.length > 1 ? "manage" : "list";
      await refresh();
    } catch (e) {
      state.error = String(e);
      render();
    }
  });
  const cancel = el("button", "btn btn-ghost", "Cancel");
  cancel.addEventListener("click", () => {
    state.error = "";
    state.confirmId = null;
    state.mode = "manage";
    render();
  });
  actions.append(remove, cancel);
  wrap.appendChild(actions);

  body.appendChild(wrap);
  afterRender();
}

// ---- actions ----
function canAdd() {
  const a = state.appByKey[state.draft.app];
  return !!a && a.installed && state.draft.label.trim().length > 0;
}

function openAdd() {
  const firstInstalled = state.apps.find((a) => a.installed);
  state.draft = {
    app: firstInstalled ? firstInstalled.key : null,
    label: "",
    color: nextColor(),
  };
  state.error = "";
  state.mode = "add";
  render();
}

function closeForm() {
  state.error = "";
  state.mode = "list";
  render();
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
    state.mode = "list";
    await refresh();
  } catch (e) {
    state.error = String(e);
    render();
  }
}

async function launchProfile(p) {
  try {
    state.error = "";
    await invoke("launch_profile", { id: p.id });
    // panel hides itself on success (Rust side)
  } catch (e) {
    state.error = String(e);
    render();
  }
}

function updateFooter() {
  const manageBtn = document.getElementById("manageBtn");
  manageBtn.textContent = state.mode === "manage" ? "Done" : "Manage";
  manageBtn.classList.toggle("active", state.mode === "manage");
  // Show "+ Add profile" whenever there's something on screen (lanes or an adopt
  // strip); hide it only on the pristine empty state, which has its own big button.
  const showAdd = state.profiles.length > 0 || state.adoptable.length > 0;
  document.getElementById("addBtn").style.display = showAdd ? "" : "none";
  // "Manage" only makes sense once there are real lanes.
  manageBtn.style.display = state.profiles.length > 0 ? "" : "none";
}

// ---- height sync ----
function afterRender() {
  requestAnimationFrame(() => {
    const panel = document.getElementById("panel");
    const h = panel.offsetHeight + 12; // account for the 6px margins
    invoke("set_panel_height", { height: h });
  });
}

// ---- keyboard ----
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (state.mode === "add") return closeForm();
    if (state.mode === "confirm") {
      state.mode = "manage";
      state.error = "";
      return render();
    }
    return invoke("hide_panel");
  }
  if (state.mode !== "list" || state.profiles.length === 0) return;
  if (e.key === "ArrowDown") {
    e.preventDefault();
    state.selected = Math.min(state.profiles.length - 1, state.selected + 1);
    render();
    focusSelected();
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    state.selected = Math.max(0, state.selected - 1);
    render();
    focusSelected();
  } else if (e.key === "Enter") {
    const p = state.profiles[state.selected];
    if (p) launchProfile(p);
  }
});

function focusSelected() {
  const rows = $body().querySelectorAll(".lane");
  if (rows[state.selected]) rows[state.selected].focus();
}

// ---- footer wiring ----
document.getElementById("addBtn").addEventListener("click", openAdd);
document.getElementById("manageBtn").addEventListener("click", () => {
  state.error = "";
  state.mode = state.mode === "manage" ? "list" : "manage";
  render();
});
document.getElementById("quitBtn").addEventListener("click", () => invoke("quit"));

// Refresh "running" state each time the panel regains focus (it's shown on tray click).
window.addEventListener("focus", () => {
  if (state.mode === "list" || state.mode === "manage") refreshRunning();
});

// ---- boot ----
refresh();
