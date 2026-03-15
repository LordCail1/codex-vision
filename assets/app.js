const state = {
  graph: null,
  scope: "current",
  activeOnly: false,
  showArchived: true,
  selectedId: null,
};

const graphEl = document.getElementById("graph");
const summaryEl = document.getElementById("summary");
const detailsEl = document.getElementById("details");
const workspaceEl = document.getElementById("workspace-banner");
const currentButton = document.getElementById("scope-current");
const allButton = document.getElementById("scope-all");
const activeOnlyInput = document.getElementById("active-only");
const showArchivedInput = document.getElementById("show-archived");

currentButton.addEventListener("click", () => {
  state.scope = "current";
  render();
});

allButton.addEventListener("click", () => {
  state.scope = "all";
  render();
});

activeOnlyInput.addEventListener("change", () => {
  state.activeOnly = activeOnlyInput.checked;
  render();
});

showArchivedInput.addEventListener("change", () => {
  state.showArchived = showArchivedInput.checked;
  render();
});

connect();

function connect() {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const ws = new WebSocket(`${protocol}//${window.location.host}/ws`);
  ws.addEventListener("message", (event) => {
    const payload = JSON.parse(event.data);
    if (payload.type === "snapshot") {
      state.graph = payload.state;
      if (!state.selectedId || !state.graph.nodes.some((node) => node.id === state.selectedId)) {
        state.selectedId = state.graph.nodes[0]?.id ?? null;
      }
      render();
    }
  });
}

function render() {
  if (!state.graph) {
    return;
  }

  const filteredNodes = state.graph.nodes.filter((node) => {
    if (state.scope === "current" && !node.workspace_match) {
      return false;
    }
    if (!state.showArchived && node.archived) {
      return false;
    }
    if (state.activeOnly && node.status !== "active") {
      return false;
    }
    return true;
  });
  const filteredIds = new Set(filteredNodes.map((node) => node.id));
  const filteredEdges = state.graph.edges.filter(
    (edge) => filteredIds.has(edge.parent_id) && filteredIds.has(edge.child_id),
  );

  updateScopeButtons();
  renderWorkspace(filteredNodes);
  renderSummary(filteredNodes);
  renderGraph(filteredNodes, filteredEdges);
  renderDetails(filteredNodes);
}

function updateScopeButtons() {
  currentButton.classList.toggle("is-active", state.scope === "current");
  allButton.classList.toggle("is-active", state.scope === "all");
}

function renderWorkspace(nodes) {
  const repoRoot = state.graph.launch_repo_root
    ? `<div><strong>Repo root</strong><code title="${escapeAttribute(state.graph.launch_repo_root)}">${escape(state.graph.launch_repo_root)}</code></div>`
    : `<div><strong>Repo root</strong><span>not detected</span></div>`;
  const workspaceMatches = state.graph.nodes.filter((node) => node.workspace_match).length;
  const visibleLabel =
    state.scope === "current"
      ? `${nodes.length} visible in current workspace`
      : `${nodes.length} visible from all sessions`;

  workspaceEl.innerHTML = `
    <div>
      <strong>Launch workspace</strong>
      <code title="${escapeAttribute(state.graph.launch_cwd)}">${escape(state.graph.launch_cwd)}</code>
    </div>
    ${repoRoot}
    <div class="workspace-meta">
      <span>${visibleLabel}</span>
      <span>${workspaceMatches} total workspace matches</span>
      <span>${state.graph.nodes.length} total scanned sessions</span>
    </div>
  `;
}

function renderSummary(nodes) {
  const counts = nodes.reduce(
    (acc, node) => {
      acc.total += 1;
      acc[node.status] += 1;
      return acc;
    },
    { total: 0, active: 0, idle: 0, archived: 0, orphaned: 0 },
  );
  summaryEl.innerHTML = `
    <span class="dot active"></span> ${counts.active}
    <span class="dot idle"></span> ${counts.idle}
    <span class="dot archived"></span> ${counts.archived}
    <span class="dot orphaned"></span> ${counts.orphaned}
    <strong>${counts.total} sessions</strong>
  `;
}

function renderGraph(nodes, edges) {
  graphEl.innerHTML = "";
  if (nodes.length === 0) {
    graphEl.setAttribute("viewBox", "0 0 800 240");
    graphEl.insertAdjacentHTML(
      "beforeend",
      `<text x="60" y="120" fill="#6e6a63" font-size="24">No sessions match the current filters.</text>`,
    );
    return;
  }

  const nodesById = new Map(nodes.map((node) => [node.id, node]));
  const children = new Map();
  for (const edge of edges) {
    if (!children.has(edge.parent_id)) {
      children.set(edge.parent_id, []);
    }
    children.get(edge.parent_id).push(nodesById.get(edge.child_id));
  }
  for (const value of children.values()) {
    value.sort(sortNodes);
  }

  const roots = nodes
    .filter((node) => !node.parent_id || !nodesById.has(node.parent_id))
    .sort(sortNodes);

  const placements = [];
  let row = 0;
  for (const root of roots) {
    row = place(root, 0, row, children, placements);
    row += 1;
  }

  const width = Math.max(...placements.map((item) => item.depth), 0) * 290 + 320;
  const height = Math.max(...placements.map((item) => item.row), 0) * 130 + 200;
  graphEl.setAttribute("viewBox", `0 0 ${width} ${height}`);

  for (const edge of edges) {
    const parent = placements.find((item) => item.node.id === edge.parent_id);
    const child = placements.find((item) => item.node.id === edge.child_id);
    if (!parent || !child) continue;
    const x1 = parent.x + 240;
    const y1 = parent.y + 44;
    const x2 = child.x;
    const y2 = child.y + 44;
    const midX = (x1 + x2) / 2;
    graphEl.insertAdjacentHTML(
      "beforeend",
      `<path d="M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}" fill="none" stroke="#b9ad95" stroke-width="3" opacity="0.9" />`,
    );
  }

  for (const item of placements) {
    const { node, x, y } = item;
    const isSelected = node.id === state.selectedId;
    const branch = escape(node.git_branch || "no-branch");
    const cwd = escape(shortPath(node.cwd || "unknown"));
    const status = node.status;
    const statusColor = statusFill(status);
    const border = isSelected ? "#17212b" : "#d5c9b3";
    const tmux = node.tmux_location ? `<tspan x="${x + 16}" dy="18">${escape(shortPath(node.tmux_location.session + " / " + node.tmux_location.window))}</tspan>` : "";
    graphEl.insertAdjacentHTML(
      "beforeend",
      `<g class="node" data-id="${node.id}">
        <rect x="${x}" y="${y}" width="240" height="88" fill="#fffaf4" stroke="${border}" />
        <rect x="${x + 14}" y="${y + 14}" width="10" height="10" rx="5" fill="${statusColor}" />
        <text x="${x + 34}" y="${y + 24}" fill="#17212b" font-size="15" font-weight="700">${escape(node.display_name)}</text>
        <text x="${x + 16}" y="${y + 46}" fill="#6e6a63" font-size="12">${branch}</text>
        <text x="${x + 16}" y="${y + 64}" fill="#6e6a63" font-size="12">${cwd}</text>
        <text x="${x + 16}" y="${y + 82}" fill="#6e6a63" font-size="11">${status.toUpperCase()}${node.active_process ? "  PID " + node.active_process.pid : ""}</text>
      </g>`,
    );
  }

  for (const element of graphEl.querySelectorAll(".node")) {
    element.addEventListener("click", () => {
      state.selectedId = element.dataset.id;
      render();
    });
  }
}

function renderDetails(nodes) {
  const node = nodes.find((item) => item.id === state.selectedId) || nodes[0];
  if (!node) {
    detailsEl.innerHTML = "<p>No session selected.</p>";
    return;
  }
  state.selectedId = node.id;
  const warningItems = (state.graph.warnings || [])
    .map((warning) => `<li>${escape(warning)}</li>`)
    .join("");

  detailsEl.innerHTML = `
    <div class="status-pill">
      <span class="dot ${node.status}"></span>
      <strong>${escape(node.display_name)}</strong>
      <span>${node.status.toUpperCase()}</span>
    </div>
    <dl style="margin-top: 18px">
      <dt>Thread</dt><dd>${escape(node.id)}</dd>
      <dt>Parent</dt><dd>${escape(node.parent_id || "root")}</dd>
      <dt>Branch</dt><dd>${escape(node.git_branch || "unknown")}</dd>
      <dt>Updated</dt><dd>${escape(formatTime(node.updated_at))}</dd>
      <dt>Workspace</dt><dd>${node.workspace_match ? "current" : "outside current workspace"}</dd>
      <dt>CWD</dt><dd>${escape(node.cwd || "unknown")}</dd>
      <dt>Repo root</dt><dd>${escape(node.repo_root || "unknown")}</dd>
      <dt>Rollout</dt><dd>${escape(node.rollout_path || "unknown")}</dd>
      <dt>PID</dt><dd>${node.active_process ? node.active_process.pid : "inactive"}</dd>
      <dt>tmux</dt><dd>${escape(formatTmux(node.tmux_location))}</dd>
    </dl>
    <div id="warnings">
      <strong>Warnings</strong>
      <ul>${warningItems || "<li>None</li>"}</ul>
    </div>
  `;
}

function place(node, depth, row, children, placements) {
  const item = { node, depth, row, x: depth * 290 + 40, y: row * 130 + 30 };
  placements.push(item);
  let nextRow = row;
  const childNodes = children.get(node.id) || [];
  for (const child of childNodes) {
    nextRow += 1;
    nextRow = place(child, depth + 1, nextRow, children, placements);
  }
  return nextRow;
}

function sortNodes(left, right) {
  const updated = (right.updated_at || 0) - (left.updated_at || 0);
  if (updated !== 0) {
    return updated;
  }
  return left.display_name.localeCompare(right.display_name);
}

function statusFill(status) {
  switch (status) {
    case "active":
      return "#1f9d74";
    case "archived":
      return "#8c8f94";
    case "orphaned":
      return "#d85050";
    default:
      return "#d6861c";
  }
}

function shortPath(value) {
  return value.length > 42 ? `...${value.slice(-39)}` : value;
}

function formatTime(value) {
  if (!value) return "unknown";
  return new Date(value * 1000).toLocaleString();
}

function formatTmux(location) {
  if (!location) return "not attached";
  return `${location.session} / ${location.window} / pane ${location.pane}`;
}

function escape(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function escapeAttribute(value) {
  return escape(value).replaceAll("'", "&#39;");
}
