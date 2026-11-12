// 8. Módulo: Usuarios & Grupos
// ==============================================================================
const PROTECTED_USERS = ['root', 'administrador', 'sistemas'];
let _usersData = [];
let _groupsData = [];
let _usersFilter = 'all';
let _activeGroup = '';
let _lastCreatedUserMemo = null;

function toggleUserGroupsExpand(btnEl) {
  if (typeof btnEl === 'string') {
    // Compatibilidad si se llama con username
    const el = document.querySelector(`.user-groups-cell[data-username="${CSS.escape(btnEl)}"] .tag-pill-more`);
    if (el) toggleUserGroupsExpand(el);
    return;
  }
  const cell = btnEl?.closest('.user-groups-cell');
  if (!cell) return;
  const moreSpan = cell.querySelector('.user-groups-extra');
  if (!moreSpan) return;
  const isHidden = moreSpan.style.display === 'none';
  moreSpan.style.display = isHidden ? 'inline' : 'none';
  btnEl.textContent = isHidden ? '« Menos' : (btnEl.dataset.originalText || '+más');
}

function renderUserGroupPills(username, groups) {
  const allGroups = (groups || []).filter(g => g.startsWith('grp_') || ['sudo', 'adm'].includes(g));
  if (!allGroups.length) {
    return '<span style="color:var(--text-muted); font-size:12px;"><em>Sin grupos</em></span>';
  }
  if (allGroups.length <= 2) {
    return `<div class="user-groups-cell" data-username="${escapeHtml(username)}">${allGroups.map(g => `<span class="tag-pill">${escapeHtml(g)}</span>`).join(' ')}</div>`;
  }
  const visible = allGroups.slice(0, 2).map(g => `<span class="tag-pill">${escapeHtml(g)}</span>`).join(' ');
  const extra = allGroups.slice(2).map(g => `<span class="tag-pill">${escapeHtml(g)}</span>`).join(' ');
  const remaining = allGroups.length - 2;
  return `
    <div class="user-groups-cell" data-username="${escapeHtml(username)}">
      ${visible}
      <span class="user-groups-extra" style="display:none;"> ${extra}</span>
      <span class="tag-pill tag-pill-more" data-original-text="+${remaining} más" onclick="toggleUserGroupsExpand(this)">+${remaining} más</span>
    </div>`;
}

function copyPasswordInput(inputId) {
  const input = document.getElementById(inputId);
  if (!input || !input.value) {
    showToast('No hay contraseña para copiar.', 'warning');
    return;
  }
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(input.value).then(() => {
      showToast('Contraseña copiada al portapapeles.', 'success');
    }).catch(() => {
      input.select();
      document.execCommand('copy');
      showToast('Contraseña copiada.', 'success');
    });
  } else {
    input.select();
    document.execCommand('copy');
    showToast('Contraseña copiada.', 'info');
  }
}

function selectRoleCard(formPrefix, role) {
  const containerId = formPrefix === 'user' ? 'new-user-role-cards' : 'edit-user-role-cards';
  const container = document.getElementById(containerId);
  if (container) {
    container.querySelectorAll('.role-card').forEach(card => {
      card.classList.toggle('active', card.dataset.role === role);
    });
  }

  const roleInput = document.getElementById(`${formPrefix}-role-val`);
  if (roleInput) {
    roleInput.value = role;
  }

  const isSuper = role === 'superadmin';
  const isAdmin = role === 'admin' || isSuper;
  const canWeb = role === 'operator' || isAdmin;

  const adminChk = document.getElementById(`${formPrefix}-is-admin`);
  if (adminChk) adminChk.checked = isAdmin;

  const webChk = document.getElementById(`${formPrefix}-can-web`);
  if (webChk) webChk.checked = canWeb;

  const superChk = document.getElementById(`${formPrefix}-is-superadmin`);
  if (superChk) superChk.checked = isSuper;

  updateEffectiveAccessPreview(formPrefix === 'edit-user');
}

function generateAndSetPassword(inputId) {
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789!@#$%&*-_';
  let pass = '';
  const array = new Uint32Array(14);
  window.crypto.getRandomValues(array);
  for (let i = 0; i < 14; i++) {
    pass += chars[array[i] % chars.length];
  }
  const el = document.getElementById(inputId);
  if (el) {
    el.value = pass;
    el.type = 'text';
    const eyeIcon = document.getElementById(`${inputId}-eye`);
    if (eyeIcon) {
      eyeIcon.innerHTML = '<use href="#icon-eye-off"></use>';
    }
    showToast('Contraseña segura generada.', 'info');
  }
}

function togglePasswordVisibility(inputId) {
  const el = document.getElementById(inputId);
  const eyeIcon = document.getElementById(`${inputId}-eye`);
  if (!el) return;
  if (el.type === 'password') {
    el.type = 'text';
    if (eyeIcon) eyeIcon.innerHTML = '<use href="#icon-eye-off"></use>';
  } else {
    el.type = 'password';
    if (eyeIcon) eyeIcon.innerHTML = '<use href="#icon-eye"></use>';
  }
}

function copyUncPath(customVal = null) {
  const val = (customVal !== null && customVal !== undefined)
    ? String(customVal)
    : (document.getElementById('welcome-unc-path')?.value || '');
  if (!val) return;

  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(val).then(() => {
      showToast(`Ruta UNC copiada: ${val}`, 'success');
    }).catch(() => {
      const input = document.getElementById('welcome-unc-path');
      if (input) {
        input.select();
        document.execCommand('copy');
      }
      showToast(`Ruta UNC copiada: ${val}`, 'success');
    });
  } else {
    const input = document.getElementById('welcome-unc-path');
    if (input) {
      input.select();
      document.execCommand('copy');
    }
    showToast(`Ruta UNC: ${val}`, 'info');
  }
}

function copyShareUnc(shareName) {
  const host = window.NAS_HOSTNAME || 'SRV-NAS';
  copyUncPath(`\\\\${host}\\${shareName}`);
}

function filterGroupMembers() {
  const query = (document.getElementById('group-member-search')?.value || '').toLowerCase().trim();
  const rows = document.querySelectorAll('#group-members-list > div');
  rows.forEach(r => {
    const text = (r.getAttribute('data-username') || r.querySelector('.member-name')?.textContent || r.textContent).toLowerCase();
    r.style.display = (!query || text.includes(query)) ? 'flex' : 'none';
  });
}

async function updateEffectiveAccessPreview(isEdit) {
  const prefix = isEdit ? 'edit-user' : 'user';
  const previewBox = document.getElementById(`${prefix}-effective-access-preview`);
  if (!previewBox) return;

  const sambaEnabled = document.getElementById(`${prefix}-samba-enabled`)?.checked ?? true;
  if (!sambaEnabled) {
    previewBox.innerHTML = '<em style="color:var(--text-muted); font-size:12px;">Acceso Samba desactivado (sin acceso a recursos de red).</em>';
    return;
  }

  if (!_permData || !_permData.shares || !_permData.shares.length) {
    try {
      const res = await apiFetch('/api/shares/access');
      _permData = res.data || { shares: [], groups: [], users: [] };
    } catch (e) {
      previewBox.innerHTML = '<em style="color:var(--text-muted); font-size:12px;">No se pudo cargar la lista de recursos compartidos.</em>';
      return;
    }
  }

  const shares = _permData.shares || [];
  if (!shares.length) {
    previewBox.innerHTML = '<em style="color:var(--text-muted); font-size:12px;">No hay recursos compartidos configurados en el servidor.</em>';
    return;
  }

  const groupCheckboxes = document.querySelectorAll(`input[name="${isEdit ? 'edit_user_group' : 'user_group'}"]:checked`);
  const selectedGroups = Array.from(groupCheckboxes).map(cb => cb.value);
  const role = document.getElementById(`${prefix}-role-val`)?.value || 'standard';
  const isAdmin = role === 'admin' || role === 'superadmin';

  const groupRows = _permData.groups || [];
  let html = '';

  shares.forEach(share => {
    let label = 'Sin acceso';
    let badgeClass = 'badge-gray';

    if (isAdmin) {
      label = 'Lectura y escritura (Admin)';
      badgeClass = 'badge-ok';
    } else {
      let level = 'none';
      for (const gname of selectedGroups) {
        const grow = groupRows.find(r => r.name === gname);
        if (grow) {
          const perm = grow[share];
          if (perm === 'write') {
            level = 'write';
            break;
          } else if (perm === 'read' && level !== 'write') {
            level = 'read';
          }
        }
      }
      if (level === 'write') {
        label = 'Lectura y escritura';
        badgeClass = 'badge-ok';
      } else if (level === 'read') {
        label = 'Solo lectura';
        badgeClass = 'badge-blue';
      }
    }

    html += `
      <div class="access-preview-item">
        <span class="share-name"><svg class="icon icon-sm"><use href="#icon-folder"></use></svg> [${escapeHtml(share)}]</span>
        <span class="badge ${badgeClass}">${label}</span>
      </div>`;
  });

  previewBox.innerHTML = html;
}

let _currentUsersTab = 'users';
let _pendingUserActionCallback = null;

function switchUsersTab(tab) {
  _currentUsersTab = tab;
  ['users', 'groups', 'perms'].forEach(t => {
    const btn = document.getElementById(`usr-tab-btn-${t}`);
    const panel = document.getElementById(`usr-panel-${t}`);
    if (btn) btn.classList.toggle('active', t === tab);
    if (panel) panel.style.display = (t === tab) ? 'block' : 'none';
  });
  if (tab === 'perms') {
    loadAccessMatrix();
  }
}

function updateUsersKpis(users, groups) {
  const uList = users || [];
  const gList = groups || [];

  const kpiTotal = document.getElementById('usr-kpi-total');
  if (kpiTotal) kpiTotal.textContent = `${uList.length} cuentas`;

  const superadmins = uList.filter(u => u.is_superadmin).length;
  const admins = uList.filter(u => u.is_admin && !u.is_superadmin).length;
  const operators = uList.filter(u => u.can_web && !u.is_admin).length;
  const standards = uList.filter(u => !u.is_admin && !u.can_web).length;

  const descParts = [];
  if (superadmins) descParts.push(`${superadmins} Superadmin`);
  if (admins) descParts.push(`${admins} Admin`);
  if (operators) descParts.push(`${operators} Operador`);
  if (standards) descParts.push(`${standards} Empleado`);

  const kpiTotalDesc = document.getElementById('usr-kpi-total-desc');
  if (kpiTotalDesc) kpiTotalDesc.textContent = descParts.join(' · ') || 'Cuentas Linux / Samba';

  const activeCount = uList.filter(u => (u.samba_enabled ?? u.enabled) !== false).length;
  const blockedCount = uList.length - activeCount;

  const kpiActive = document.getElementById('usr-kpi-active');
  if (kpiActive) {
    kpiActive.textContent = `${activeCount} Activas`;
    kpiActive.style.color = blockedCount > 0 ? 'var(--accent-warning)' : 'var(--accent-success-text)';
  }

  const kpiBlockedDesc = document.getElementById('usr-kpi-blocked-desc');
  if (kpiBlockedDesc) {
    kpiBlockedDesc.textContent = blockedCount > 0 ? `${blockedCount} cuenta(s) suspendida(s)` : 'Todas las cuentas operativas';
  }

  const sambaCount = uList.filter(u => !!u.is_samba).length;
  const noSambaCount = uList.length - sambaCount;

  const kpiSamba = document.getElementById('usr-kpi-samba');
  if (kpiSamba) kpiSamba.textContent = `${sambaCount} con SMB`;

  const kpiNoSambaDesc = document.getElementById('usr-kpi-nosamba-desc');
  if (kpiNoSambaDesc) kpiNoSambaDesc.textContent = noSambaCount > 0 ? `${noSambaCount} sin sincronización Samba` : '100% sincronizados en Samba';

  const groupsWithMembers = gList.filter(g => (g.members || []).length > 0).length;
  const kpiGroups = document.getElementById('usr-kpi-groups');
  if (kpiGroups) kpiGroups.textContent = `${gList.length} grupos`;

  const kpiGroupsDesc = document.getElementById('usr-kpi-groups-desc');
  if (kpiGroupsDesc) kpiGroupsDesc.textContent = `${groupsWithMembers} con miembros asignados`;

  const tabUserCount = document.getElementById('usr-tab-count');
  if (tabUserCount) tabUserCount.textContent = String(uList.length);

  const tabGroupCount = document.getElementById('grp-tab-count');
  if (tabGroupCount) tabGroupCount.textContent = String(gList.length);

  updateUserChipCounters(uList);
}

function updateUserChipCounters(users) {
  const uList = users || [];
  const cAll = document.getElementById('chip-count-all');
  if (cAll) cAll.textContent = String(uList.length);

  const cAdmin = document.getElementById('chip-count-admin');
  if (cAdmin) cAdmin.textContent = String(uList.filter(u => u.is_admin || u.is_superadmin).length);

  const cOperator = document.getElementById('chip-count-operator');
  if (cOperator) cOperator.textContent = String(uList.filter(u => u.can_web && !u.is_admin).length);

  const cStandard = document.getElementById('chip-count-standard');
  if (cStandard) cStandard.textContent = String(uList.filter(u => !u.is_admin && !u.can_web).length);

  const cBlocked = document.getElementById('chip-count-blocked');
  if (cBlocked) cBlocked.textContent = String(uList.filter(u => (u.samba_enabled ?? u.enabled) === false).length);

  const cSamba = document.getElementById('chip-count-samba');
  if (cSamba) cSamba.textContent = String(uList.filter(u => !!u.is_samba).length);
}

function showUserActionConfirm({ title, icon = 'alert-triangle', alertText, alertType = 'warning', desc, prompt = '¿Deseas proceder con esta acción?', btnText = 'Confirmar', btnClass = 'btn-danger', onConfirm }) {
  const titleEl = document.getElementById('user-action-confirm-title-text');
  if (titleEl) titleEl.textContent = title || 'Confirmar acción';

  const iconEl = document.getElementById('user-action-confirm-icon');
  if (iconEl) {
    iconEl.innerHTML = `<use href="#icon-${escapeHtml(icon)}"></use>`;
    iconEl.style.color = alertType === 'danger' ? 'var(--accent-danger)' : 'var(--accent-warning)';
  }

  const alertBox = document.getElementById('user-action-confirm-alert');
  const alertTextEl = document.getElementById('user-action-confirm-alert-text');
  if (alertBox && alertTextEl) {
    alertBox.className = `alert-box alert-box-${alertType}`;
    alertTextEl.textContent = alertText || '';
  }

  const descEl = document.getElementById('user-action-confirm-desc');
  if (descEl) descEl.textContent = desc || '';

  const promptEl = document.getElementById('user-action-confirm-prompt');
  if (promptEl) promptEl.textContent = prompt;

  const submitBtn = document.getElementById('btn-user-action-confirm-submit');
  if (submitBtn) {
    submitBtn.textContent = btnText;
    submitBtn.className = `btn ${btnClass}`;
  }

  _pendingUserActionCallback = onConfirm;
  openModal('modal-user-action-confirm');
}

async function executeUserActionConfirm() {
  if (typeof _pendingUserActionCallback !== 'function') {
    closeModal('modal-user-action-confirm');
    return;
  }
  const cb = _pendingUserActionCallback;
  _pendingUserActionCallback = null;
  closeModal('modal-user-action-confirm');
  await cb();
}

let _usersSortCol = 'username';
let _usersSortAsc = true;

function sortUsersTable(col) {
  if (_usersSortCol === col) {
    _usersSortAsc = !_usersSortAsc;
  } else {
    _usersSortCol = col;
    _usersSortAsc = true;
  }

  ['username', 'uid', 'role', 'status'].forEach(c => {
    const icon = document.getElementById(`sort-icon-${c}`);
    if (icon) {
      icon.textContent = (c === col) ? (_usersSortAsc ? '▲' : '▼') : '↕';
      icon.style.opacity = (c === col) ? '1' : '0.5';
    }
  });

  _usersData.sort((a, b) => {
    let valA, valB;
    if (col === 'uid') {
      valA = a.uid ?? 0;
      valB = b.uid ?? 0;
      return _usersSortAsc ? (valA - valB) : (valB - valA);
    } else if (col === 'role') {
      const score = u => u.is_superadmin ? 4 : (u.is_admin ? 3 : (u.can_web ? 2 : 1));
      valA = score(a);
      valB = score(b);
      return _usersSortAsc ? (valB - valA) : (valA - valB);
    } else if (col === 'status') {
      valA = (a.samba_enabled ?? a.enabled) ? 1 : 0;
      valB = (b.samba_enabled ?? b.enabled) ? 1 : 0;
      return _usersSortAsc ? (valB - valA) : (valA - valB);
    } else {
      valA = (a.username || '').toLowerCase();
      valB = (b.username || '').toLowerCase();
      return _usersSortAsc ? valA.localeCompare(valB) : valB.localeCompare(valA);
    }
  });

  renderUsersTableBody();
}

function renderUsersTableBody() {
  const usersTbody = document.getElementById('users-table-body');
  if (!usersTbody) return;

  const isAdminCaller = window.NAS_IS_ADMIN !== false;
  usersTbody.innerHTML = _usersData.map(u => {
    const isProtected = PROTECTED_USERS.includes(u.username) || !!u.is_superadmin;
    const isStandard = !u.is_admin && !u.can_web;
    const rol = u.is_superadmin
      ? '<span class="badge badge-err">Superadmin</span>'
      : (u.is_admin
        ? '<span class="badge badge-warn">Admin</span>'
        : (u.can_web ? '<span class="badge badge-blue">Operador</span>' : '<span class="badge badge-gray">Estándar</span>'));
    const netEnabled = !!(u.samba_enabled ?? u.enabled);
    const canManage = !isProtected && (isAdminCaller || !u.is_admin);

    const shellBadge = (u.shell && u.shell.includes('nologin'))
      ? '<span class="badge badge-gray" title="Sin acceso interactivo (cuenta de red Samba)">Solo Red (nologin)</span>'
      : `<span class="badge badge-ok" title="Shell interactiva habilitada">Terminal (${escapeHtml(u.shell || '/bin/bash')})</span>`;

    return `
    <tr data-username="${escapeHtml(u.username)}" data-fullname="${escapeHtml(u.full_name || '')}" data-admin="${(u.is_admin || u.is_superadmin) ? '1' : '0'}" data-operator="${(u.can_web && !u.is_admin) ? '1' : '0'}" data-standard="${isStandard ? '1' : '0'}" data-enabled="${netEnabled ? '1' : '0'}" data-samba="${u.is_samba ? '1' : '0'}" data-uid="${u.uid}" data-groups="${escapeHtml((u.groups || []).join(' '))}" data-shell="${escapeHtml(u.shell || '')}">
      <td><strong>${escapeHtml(u.username)}</strong><div style="font-size:11px; color:var(--text-muted);">${escapeHtml(u.full_name || 'Sin nombre asignado')}</div></td>
      <td><code>${u.uid}</code></td>
      <td>${rol}</td>
      <td>${shellBadge}</td>
      <td>${renderUserGroupPills(u.username, u.groups)}</td>
      <td>${netEnabled ? '<span class="badge badge-ok">Activo</span>' : '<span class="badge badge-err">Suspendido</span>'}</td>
      <td>${u.is_samba ? (u.samba_enabled !== false ? '<span class="badge badge-ok">Sync</span>' : '<span class="badge badge-warn">SMB Deshab</span>') : '<span class="badge badge-gray">Sin SMB</span>'}</td>
      <td style="text-align:right; white-space:nowrap;">
        ${canManage ? `
          <button class="btn btn-secondary btn-sm" title="Editar" onclick="openEditUser('${escapeHtml(u.username)}')"><svg class="icon icon-sm"><use href="#icon-edit"></use></svg></button>
          <button class="btn btn-secondary btn-sm" title="${netEnabled ? 'Suspender acceso a red' : 'Reactivar acceso a red'}" onclick="toggleUser('${escapeHtml(u.username)}', ${netEnabled ? 'false' : 'true'})"><svg class="icon icon-sm"><use href="#icon-${netEnabled ? 'lock' : 'unlock'}"></use></svg></button>
          <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" title="Eliminar" onclick="deleteUser('${escapeHtml(u.username)}')"><svg class="icon icon-sm"><use href="#icon-trash"></use></svg></button>
        ` : `<span class="badge badge-gray">${isProtected ? (u.is_superadmin ? 'Inmutable' : 'Protegida') : 'Solo lectura'}</span>`}
      </td>
    </tr>`;
  }).join('') || '<tr><td colspan="8" style="text-align:center;">Sin usuarios encontrados.</td></tr>';

  applyUsersFilters();
}
