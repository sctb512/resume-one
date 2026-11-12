// Trusted host code. The upstream iframe has no scripts or IPC capabilities.
const editPanel = document.querySelector("#edit-panel");
const editHit = document.querySelector("#paper-hit");
const editTitle = editPanel.querySelector("h2");
const editFields = editPanel.querySelector(".fields");
const editError = editPanel.querySelector(".error");
const normalize = value => value.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase()
  .replace(/[^\p{L}\p{N}]+/gu, " ").trim();
const aliases = Object.fromEntries(Object.entries({
  basics: ["about", "about me", "summary", "professional summary", "profile", "contact", "contact details", "location", "personal information", "informations", "header", "resume header", "masthead", "简介", "基本信息"],
  work: ["work", "experience", "work experience", "professional experience", "experiences professionnelles", "employment", "employment history", "工作经历"],
  education: ["education", "formation", "academic background", "教育经历"],
  projects: ["projects", "projects experience", "project experience", "notable projects", "research projects", "open source projects", "projets", "项目经历"],
  skills: ["skills", "technical skills", "competences", "技能"],
  volunteer: ["volunteer", "volunteer work", "volunteer experience", "service leadership", "benevolat", "志愿经历"],
  awards: ["awards", "awards honors", "honors", "honours", "recompenses", "荣誉奖项"],
  certificates: ["certificates", "certifications", "证书"],
  publications: ["publications", "出版物"],
  languages: ["languages", "langues", "语言"],
  interests: ["interests", "research interests", "soft skills", "centres d interet", "兴趣"],
  references: ["references", "推荐人"],
}).flatMap(([id, names]) => [id, ...names].map(name => [name, id])));
const headings = "h1,h2,h3,h4,h5,h6,[role=heading]";
const entryBlocks = "article,.item,.section-item,.pro-entry,.entry,.detail-item,.skills-item,.list-group-item,tr";
let editEnabled = false, editTheme = "", editToken = 0, session = 0;
let regions = [], edited = null, candidate = null, timer, suppressUntil = 0;
let suppressedTarget = null;
let pendingJump = null, lastReading = "";
const pendingFields = new Map();

function sectionFor(element) {
  const id = normalize(element.id || "").replace(/^pro /, "");
  return Object.hasOwn(aliases, id) ? aliases[id]
    : element.matches(headings) && Object.hasOwn(aliases, normalize(element.textContent))
      ? aliases[normalize(element.textContent)] : null;
}

function documentRegions(doc, targets) {
  const sectionHeads = new Map(), text = new Map();
  const elements = doc.querySelectorAll("body *:not(svg):not(svg *):not(style)");
  const boxes = element => {
    const rect = element.getBoundingClientRect();
    if (rect.width || rect.height || !element.children.length) return rect;
    // A few templates use display:contents for the printable section grid.
    const children = [...element.children].map(child => child.getBoundingClientRect())
      .filter(box => box.width && box.height);
    if (!children.length) return rect;
    const left = Math.min(...children.map(box => box.left));
    const right = Math.max(...children.map(box => box.right));
    const top = Math.min(...children.map(box => box.top));
    const bottom = Math.max(...children.map(box => box.bottom));
    return {left, right, top, bottom, width:right-left, height:bottom-top};
  };
  const visible = element => {
    const rect = boxes(element);
    return rect.width > 0 && rect.height > 0;
  };
  for (const element of elements) {
    const id = sectionFor(element);
    if (id && visible(element)) {
      const existing = sectionHeads.get(id) || [];
      existing.push(element);
      sectionHeads.set(id, existing);
    }
    if (element.childElementCount <= 3 && element.textContent.length <= 360 && visible(element)) {
      const value = normalize(element.textContent);
      if (value) text.set(element, value);
    }
  }
  const used = new Set(), output = [], bySection = new Map();
  for (const item of targets) {
    const group = bySection.get(item.section) || [];
    group.push(item);
    bySection.set(item.section, group);
  }
  for (const [section, group] of bySection) {
    const sections = sectionHeads.get(section) || [];
    const root = sections.find(el => el.matches("section,[id]:not(h1,h2,h3,h4,h5,h6)"))
      || sections[0]?.closest("section")
      || sections[0]?.parentElement;
    if (section === "work" && doc.documentElement.dataset.jianliTheme === "original-paper-plus-plus" && root) {
      const rows = [root, ...root.parentElement.querySelectorAll(":scope > section.work-content")];
      if (rows.length === group.length) {
        rows.forEach((row, index) => {
          const rect = boxes(row);
          output.push({target:group[index], left:rect.left, right:rect.right, top:rect.top, bottom:rect.bottom, anchor:row});
        });
        continue;
      }
    }
    const otherHeads = [...sectionHeads].filter(([id]) => id !== section).flatMap(([, nodes]) => nodes);
    const anchors = group.map(target => {
      if (section === "basics") {
        const name = normalize(target.anchors[0] || "");
        const title = [...text].find(([el, value]) => name && value.includes(name) &&
          el.matches("h1,header h2,[itemprop=name]"))?.[0];
        return title || sections[0];
      }
      const names = target.anchors.map(normalize).filter(Boolean);
      const options = [...text].filter(([element, value]) =>
        !used.has(element) && !sectionFor(element) && names.some(name =>
          value === name || (name.length > 3 && value.includes(name) && value.length <= name.length + 80)));
      options.sort(([a, aa], [b, bb]) => {
        const score = (el, value) => (root?.contains(el) ? 12 : 0) + (el.matches(headings) ? 4 : 0)
          + (names.includes(value) ? 4 : 0) + (el.childElementCount === 0 ? 1 : 0);
        return score(b, bb) - score(a, aa);
      });
      const anchor = options[0]?.[0] || (group.length === 1 ? sections[0] : null);
      if (anchor) {
        used.add(anchor);
        // An adjacent parent/child often repeats the very same heading.
        for (const [element] of options) if (anchor.contains(element) || element.contains(anchor)) used.add(element);
      }
      return anchor;
    });
    for (const [i, target] of group.entries()) {
      const anchor = anchors[i];
      if (!anchor) continue;
      let container = root;
      if (!container?.contains(anchor)) {
        const first = sections[0];
        container = anchor.closest(entryBlocks);
        if (first) {
          let parent = anchor.parentElement;
          while (parent && parent !== doc.body && !parent.contains(first)) parent = parent.parentElement;
          if (parent && parent !== doc.body && !otherHeads.some(el => parent.contains(el))) container = parent;
        }
        container ||= anchor.parentElement;
      }
      if (section === "basics") container = anchor.closest("header,#photo-header,.profile-card-wrapper") || container;
      const containerBox = boxes(container), anchorBox = boxes(anchor);
      const peerBoxes = anchors.filter(Boolean).filter(peer => peer !== anchor).map(boxes);
      let block = anchor.closest(entryBlocks);
      if (!block || !container.contains(block) || anchors.some(peer => peer && peer !== anchor && block.contains(peer))) block = null;
      const blockBox = block && boxes(block);
      const sameRow = peerBoxes.some(box => Math.abs(box.top - anchorBox.top) < 8);
      const later = Math.min(...peerBoxes.filter(box => box.top > anchorBox.top + 8).map(box => box.top));
      const earlier = peerBoxes.some(box => box.top < anchorBox.top - 8);
      const side = sameRow && (blockBox || anchorBox);
      const left = side ? side.left : containerBox.left;
      const right = side ? side.right : containerBox.right;
      const top = earlier || sameRow ? (blockBox || anchorBox).top : Math.min(containerBox.top, anchorBox.top);
      const bottom = Math.max(anchorBox.bottom + 4, Math.min(later, (blockBox || containerBox).bottom));
      output.push({ target, left, top, right, bottom, anchor });
      if (section === "basics") for (const el of sections) {
        if (container.contains(el)) continue;
        const extra = el.closest("section") || el;
        const rect = boxes(extra);
        output.push({ target, left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom, anchor: el });
      }
    }
  }
  return output.sort((a, b) => (a.right-a.left)*(a.bottom-a.top) - (b.right-b.left)*(b.bottom-b.top));
}

const fieldId = field => field.key + ":" + (field.index ?? "");
const targetId = target => target.section + ":" + (target.index ?? "");

function sendField(field, value) {
  if (!edited) return;
  const message = JSON.stringify({kind:"edit", token:editToken, session, section:edited.section,
    item:edited.index, field:field.key, index:field.index, value});
  if (new TextEncoder().encode(message).length > 90 * 1024) {
    editError.textContent = document.documentElement.lang === "en"
      ? "This field is too long. Edit it in JSON view." : "字段过长，请在 JSON 视图中编辑。";
    return;
  }
  window.ipc.postMessage(message);
}

function flushFields() {
  for (const { timer, field, input } of pendingFields.values()) {
    clearTimeout(timer);
    sendField(field, input.value);
  }
  pendingFields.clear();
}

function closeEdit() {
  clearTimeout(timer);
  candidate = null;
  if (!edited) return;
  flushFields();
  suppressedTarget = targetId(edited);
  edited = null;
  editPanel.inert = true;
  editPanel.classList.remove("shown");
}

function openEdit(region, point, keyboard = false) {
  if (!editEnabled || edited) return;
  edited = region.target;
  session++;
  editError.textContent = "";
  editTitle.textContent = edited.title + (edited.index === null ? "" : " · " + (edited.index + 1));
  editFields.replaceChildren();
  for (const field of edited.fields) {
    const label = document.createElement("label");
    label.append(document.createTextNode(field.label + (field.index === null ? "" : " " + (field.index + 1))));
    const input = document.createElement(field.long ? "textarea" : "input");
    label.classList.toggle("wide", field.long);
    input.value = field.value;
    input.readOnly = field.readOnly;
    input.maxLength = 48 * 1024;
    input.dataset.field = fieldId(field);
    if (field.readOnly) input.placeholder = document.documentElement.lang === "en" ? "Edit in JSON view" : "在 JSON 视图中编辑";
    label.append(input);
    editFields.append(label);
    const queue = () => {
      clearTimeout(pendingFields.get(fieldId(field))?.timer);
      const timer = setTimeout(() => {
        pendingFields.delete(fieldId(field));
        sendField(field, input.value);
      }, 155);
      pendingFields.set(fieldId(field), { timer, field, input });
    };
    input.addEventListener("input", event => { if (!event.isComposing) queue(); });
    input.addEventListener("compositionend", queue);
  }
  editPanel.inert = false;
  editPanel.classList.add("shown");
  const viewport = current.getBoundingClientRect();
  const scale = viewport.width / PAPER_WIDTH;
  const y = viewport.top + (point?.y ?? region.top) * scale;
  const start = Math.min(y - 18, innerHeight - editPanel.offsetHeight - 10);
  editPanel.style.top = Math.max(0, Math.max(6, start) - viewport.top) + "px";
  if (keyboard) editFields.querySelector("input,textarea")?.focus({preventScroll:true});
}

function installEditing(page, targets, token, theme) {
  if (theme !== editTheme) {
    closeEdit();
    suppressedTarget = null;
  }
  editToken = token;
  editTheme = theme;
  regions = documentRegions(page, targets);
  editHit.hidden = !editEnabled;
  page.fonts?.ready.then(() => {
    if (current?.contentDocument === page) regions = documentRegions(page, targets);
  });
  if (edited) {
    const next = targets.find(target => targetId(target) === targetId(edited));
    if (!next) closeEdit();
    else for (const field of next.fields) {
      const input = [...editFields.querySelectorAll("input,textarea")].find(el => el.dataset.field === fieldId(field));
      if (input && input.value !== field.value && !pendingFields.has(fieldId(field))) input.value = field.value;
    }
  }
}

// WKWebView does not consistently deliver events through a script-disabled
// iframe. The host owns the hit layer and maps the pointer to the unscaled DOM.
function findEditRegion(event) {
  if (!current) return {};
  const bounds = current.getBoundingClientRect();
  const scale = bounds.width / PAPER_WIDTH;
  const x = (event.clientX - bounds.left) / scale;
  const y = (event.clientY - bounds.top) / scale;
  return {
    y,
    region: regions.find(region => x >= region.left && x <= region.right && y >= region.top && y <= region.bottom)
  };
}
editHit.addEventListener("mousemove", event => {
  if (!editEnabled || edited || event.buttons || performance.now() < suppressUntil) return;
  const {region, y} = findEditRegion(event);
  if (region && targetId(region.target) === suppressedTarget) return;
  suppressedTarget = null;
  if (region === candidate) return;
  clearTimeout(timer);
  candidate = region;
  if (region) timer = setTimeout(() => openEdit(region, {y}), 130);
}, {passive:true});
editHit.addEventListener("mouseleave", () => {
  clearTimeout(timer);
  candidate = null;
  if (!edited) suppressedTarget = null;
});
editHit.addEventListener("click", event => {
  const {region, y} = findEditRegion(event);
  if (region && editEnabled) openEdit(region, {y}, true);
});
addEventListener("keydown", event => {
  if (event.key === "Alt") {
    closeEdit();
    paper.classList.add("selecting");
  }
});
function stopSelecting() { paper.classList.remove("selecting"); }
addEventListener("keyup", stopSelecting);
addEventListener("blur", stopSelecting);

editPanel.addEventListener("pointerleave", () => {
  suppressUntil = performance.now() + 120;
  closeEdit();
});
editPanel.querySelector("button").addEventListener("click", closeEdit);
function reportReading() {
  if (!current) return;
  const frame = current.getBoundingClientRect();
  const scale = frame.width / PAPER_WIDTH;
  const top = (36 - frame.top) / scale;
  const region = regions.reduce((best, item) =>
    !best || Math.abs(item.top - top) < Math.abs(best.top - top) ? item : best, null);
  const reading = JSON.stringify(region && scrollY > 24
    ? [region.target.section, region.target.index ?? null] : null);
  if (reading !== lastReading) window.ipc.postMessage("reading:" + reading);
  lastReading = reading;
}

function applyJump() {
  if (!current || !pendingJump) return;
  const [section, index] = pendingJump;
  const matches = regions.filter(region => region.target.section === section);
  const region = matches.find(item => (item.target.index ?? null) === index) || matches[0];
  if (region) {
    const frame = current.getBoundingClientRect();
    window.scrollTo(0, scrollY + frame.top + region.top * frame.width / PAPER_WIDTH - 36);
  }
}

window.jianliJump = (section, index) => {
  pendingJump = [section, index];
  applyJump();
  if (current && !pending) pendingJump = null;
};

addEventListener("scroll", () => {
  suppressUntil = performance.now() + 140;
  closeEdit();
  reportReading();
}, {passive:true});
window.jianliEditEnabled = enabled => {
  editEnabled = enabled;
  editHit.hidden = !enabled || !current;
  if (!enabled) closeEdit();
};
window.jianliEditResult = (forSession, error) => {
  if (forSession === session) editError.textContent = error || "";
};
