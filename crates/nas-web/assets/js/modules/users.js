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

async function loadUsersAndGroups() {
  const groupsTbody = document.getElementById('groups-table-body');

  try {
    const [uRes, gRes] = await Promise.all([
      apiFetch('/api/users'),
      apiFetch('/api/groups'),
    ]);

    _usersData = uRes.data || [];
    _groupsData = gRes.data || [];

    // Actualizar tarjetas de KPI de usuarios y grupos
    updateUsersKpis(_usersData, _groupsData);

    renderUsersTableBody();

    if (groupsTbody) {
      const isSuperCaller = !!window.NAS_IS_SUPER;
      const isAdminCaller = window.NAS_IS_ADMIN !== false;

      groupsTbody.innerHTML = _groupsData.map(g => {
        const isSpecial = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
        let canManageMembers = false;
        if (g.name === 'grp_superadmin') {
          canManageMembers = isSuperCaller;
        } else if (isSpecial) {
          canManageMembers = isAdminCaller;
        } else {
          canManageMembers = isAdminCaller;
        }

        const membersHtml = (g.members || []).map(m => {
          if (m === 'www-data') {
            return `<span class="tag-pill" style="border:1px solid var(--accent-primary); color:var(--accent-primary); font-weight:600;" title="Cuenta del servicio web necesaria para el panel">www-data (sistema)</span>`;
          }
          return `<span class="tag-pill">${escapeHtml(m)}</span>`;
        }).join(' ') || '<em style="color:var(--text-muted);">Sin miembros</em>';

        return `
        <tr data-group="${escapeHtml(g.name)}" data-members="${escapeHtml((g.members || []).join(' '))}">
          <td><strong>${escapeHtml(g.name)}</strong></td>
          <td><code>${g.gid}</code></td>
          <td>${isSpecial ? '<span class="badge badge-primary">Especial (Sistema)</span>' : '<span class="badge badge-gray">Departamental</span>'}</td>
          <td>${membersHtml}</td>
          <td style="text-align:right; white-space:nowrap;">
            ${canManageMembers ? `
              <button class="btn btn-secondary btn-sm" title="Gestionar Miembros" onclick="openGroupMembers('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-users"></use></svg></button>
            ` : ''}
            ${!isSpecial && isAdminCaller ? `
              <button class="btn btn-secondary btn-sm" title="Renombrar" onclick="openRenameGroup('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-edit"></use></svg></button>
              <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" title="Eliminar" onclick="deleteGroup('${escapeHtml(g.name)}')"><svg class="icon icon-sm"><use href="#icon-trash"></use></svg></button>
            ` : ''}
            ${!canManageMembers && (!isAdminCaller || isSpecial) && !isSpecial ? '<span class="badge badge-gray">Solo lectura</span>' : ''}
          </td>
        </tr>
      `;
      }).join('') || '<tr><td colspan="5" style="text-align:center;">Sin grupos creados.</td></tr>';
      applyGroupsFilters();
    }

    populateUserGroupOptions(_groupsData);
  } catch (e) {
    console.error('Error cargando usuarios y grupos:', e);
  }
}

function populateUserGroupOptions(groups) {
  const container = document.getElementById('user-groups-list');
  if (!container) return;

  const list = groups || [];
  if (!list.length) {
    container.innerHTML = '<em style="color:var(--text-muted);">No hay grupos disponibles. ' +
      '<a href="#" onclick="event.preventDefault(); closeModal(\'modal-new-user\'); openModal(\'modal-new-group\');">Crear un grupo</a>.</em>';
    return;
  }

  container.innerHTML = list.map(g => {
    const special = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
    return `<label style="display:flex; align-items:center; gap:8px; ${special ? 'opacity:0.65;' : ''}">
      <input type="checkbox" name="user_group" value="${escapeHtml(g.name)}" ${special ? 'disabled' : ''} onchange="updateEffectiveAccessPreview(false)">
      <span>${escapeHtml(g.name)}${special ? ' <span class="badge badge-gray">Especial</span>' : ''}</span>
    </label>`;
  }).join('');
}

function setUsersFilter(filter) {
  _usersFilter = filter;
  document.querySelectorAll('#users-filter-chips .chip-btn').forEach(b => {
    b.classList.toggle('active', b.dataset.filter === filter);
  });
  applyUsersFilters();
}

function applyUsersFilters() {
  const q = (document.getElementById('users-search')?.value || '').toLowerCase().trim();
  let visibleCount = 0;
  document.querySelectorAll('#users-table-body tr[data-username]').forEach(tr => {
    let ok = true;
    if (_usersFilter === 'admin') ok = tr.dataset.admin === '1';
    else if (_usersFilter === 'operator') ok = tr.dataset.operator === '1';
    else if (_usersFilter === 'standard') ok = tr.dataset.standard === '1';
    else if (_usersFilter === 'blocked') ok = tr.dataset.enabled !== '1';
    else if (_usersFilter === 'samba') ok = tr.dataset.samba === '1';

    if (ok && q) {
      const uName = (tr.dataset.username || '').toLowerCase();
      const fName = (tr.dataset.fullname || '').toLowerCase();
      const uid = (tr.dataset.uid || '').toLowerCase();
      const groups = (tr.dataset.groups || '').toLowerCase();
      const shell = (tr.dataset.shell || '').toLowerCase();
      ok = uName.includes(q) || fName.includes(q) || uid.includes(q) || groups.includes(q) || shell.includes(q);
    }
    tr.style.display = ok ? '' : 'none';
    if (ok) visibleCount++;
  });

  let noRes = document.getElementById('users-no-results');
  if (!noRes) {
    const tbody = document.getElementById('users-table-body');
    if (tbody) {
      tbody.insertAdjacentHTML('beforeend', '<tr id="users-no-results" style="display:none;"><td colspan="8" style="text-align:center; padding:24px; color:var(--text-muted);"><svg class="icon icon-lg" style="margin-bottom:8px;"><use href="#icon-users"></use></svg><div>No se encontraron cuentas que coincidan con el filtro.</div></td></tr>');
      noRes = document.getElementById('users-no-results');
    }
  }
  if (noRes) {
    noRes.style.display = (visibleCount === 0 && (_usersData.length > 0)) ? '' : 'none';
  }
}

function applyGroupsFilters() {
  const q = (document.getElementById('groups-search')?.value || '').toLowerCase().trim();
  document.querySelectorAll('#groups-table-body tr[data-group]').forEach(tr => {
    let ok = true;
    if (q) {
      const gname = (tr.dataset.group || '').toLowerCase();
      const members = (tr.dataset.members || '').toLowerCase();
      ok = gname.includes(q) || members.includes(q);
    }
    tr.style.display = ok ? '' : 'none';
  });
}

function openEditUser(username) {
  const u = _usersData.find(x => x.username === username);
  if (!u) return;
  document.getElementById('edit-user-uname').value = username;
  document.getElementById('edit-user-fullname').value = u.full_name || '';
  document.getElementById('edit-user-pass').value = '';
  document.getElementById('edit-user-samba-enabled').checked = !!(u.samba_enabled ?? u.enabled);

  // Determinar rol
  let userRole = 'standard';
  if (u.is_superadmin) {
    userRole = 'superadmin';
  } else if (u.is_admin) {
    userRole = 'admin';
  } else if (u.can_web) {
    userRole = 'operator';
  }
  selectRoleCard('edit-user', userRole);

  const editCards = document.getElementById('edit-user-role-cards');
  if (editCards) {
    editCards.querySelectorAll('.role-card').forEach(card => {
      if (u.is_superadmin) {
        card.classList.toggle('disabled', card.dataset.role !== 'superadmin');
      } else {
        card.classList.remove('disabled');
      }
    });
  }

  const checked = (u.groups || []).filter(g => g.startsWith('grp_'));
  const container = document.getElementById('edit-user-groups-list');
  container.innerHTML = _groupsData.map(g => {
    const special = !!g.is_special || g.name === 'grp_samba' || g.name === 'grp_web' || g.name === 'grp_superadmin' || g.name === 'grp_backups';
    const sel = (!special && checked.includes(g.name)) ? 'checked' : '';
    return `<label style="display:flex; align-items:center; gap:8px; ${special ? 'opacity:0.65;' : ''}">
      <input type="checkbox" name="edit_user_group" value="${escapeHtml(g.name)}" ${sel} ${special ? 'disabled' : ''} onchange="updateEffectiveAccessPreview(true)">
      <span>${escapeHtml(g.name)}${special ? ' <span class="badge badge-gray">Especial</span>' : ''}</span>
    </label>`;
  }).join('');

  updateEffectiveAccessPreview(true);
  openModal('modal-edit-user');
}

async function submitUpdateUser(event) {
  event.preventDefault();
  const username = document.getElementById('edit-user-uname').value;
  const full_name = document.getElementById('edit-user-fullname').value;
  const password = document.getElementById('edit-user-pass').value;
  const role = document.getElementById('edit-user-role-val')?.value || 'standard';
  const isAdmin = role === 'admin' || role === 'superadmin';
  const canWeb = role === 'operator' || isAdmin;
  const isSuperadmin = role === 'superadmin';
  const sambaEnabled = document.getElementById('edit-user-samba-enabled').checked;
  const groups = Array.from(document.querySelectorAll('input[name="edit_user_group"]:checked')).map(cb => cb.value);

  const btn = document.getElementById('btn-submit-edit-user') || event.target.querySelector('button[type="submit"]');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }

  try {
    await apiFetch('/api/users/update', {
      method: 'POST',
      body: JSON.stringify({ username, full_name, password, groups, is_admin: isAdmin, can_web: canWeb, samba_enabled: sambaEnabled, is_superadmin: isSuperadmin }),
    });
    showToast(`Usuario [${username}] actualizado.`, 'success');
    closeModal('modal-edit-user');
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

async function toggleUser(username, enabled) {
  const verb = enabled ? 'Reactivar' : 'Suspender';
  showUserActionConfirm({
    title: `${verb} cuenta [${username}]`,
    icon: enabled ? 'unlock' : 'lock',
    alertType: enabled ? 'info' : 'warning',
    alertText: enabled
      ? `Se reanudará el acceso de red a carpetas compartidas Samba y servicios para [${username}].`
      : `La cuenta [${username}] perderá acceso inmediato a recursos Samba y sesiones autenticadas.`,
    desc: `El estado se actualizará tanto en el sistema operativo Linux como en la base de datos de usuarios Samba (smbpasswd).`,
    prompt: `¿Confirmas que deseas ${verb.toLowerCase()} la cuenta [${username}]?`,
    btnText: `${verb} cuenta`,
    btnClass: enabled ? 'btn-primary' : 'btn-danger',
    onConfirm: async () => {
      try {
        await apiFetch('/api/users/toggle', {
          method: 'POST',
          body: JSON.stringify({ username, enabled }),
        });
        showToast(`Cuenta [${username}] ${enabled ? 'reactivada exitosamente' : 'suspendida'}.`, 'success');
        loadUsersAndGroups();
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    },
  });
}

function openGroupMembers(group) {
  _activeGroup = group;
  const g = _groupsData.find(x => x.name === group);
  const members = g ? (g.members || []) : [];
  document.getElementById('modal-group-members-title').textContent = `Miembros del grupo ${group} (${members.length})`;
  const searchInput = document.getElementById('group-member-search');
  if (searchInput) searchInput.value = '';

  const list = document.getElementById('group-members-list');
  list.innerHTML = members.length
    ? members.map(m => {
        if (m === 'www-data') {
          return `
            <div style="display:flex; align-items:center; justify-content:space-between; gap:8px; padding:6px 8px; background:var(--bg-surface); border-radius:6px;" data-username="${escapeHtml(m)}">
              <span class="member-name"><strong>${escapeHtml(m)}</strong> <span class="badge badge-primary">Sistema (Protegido)</span></span>
              <span style="font-size:11px; color:var(--text-muted);">Requerido por servicio web</span>
            </div>`;
        }
        return `
          <div style="display:flex; align-items:center; justify-content:space-between; gap:8px; padding:4px 6px; background:var(--bg-surface); border-radius:6px;" data-username="${escapeHtml(m)}">
            <span class="member-name">${escapeHtml(m)}</span>
            <button class="btn btn-secondary btn-sm" style="color:var(--accent-danger);" onclick="removeGroupMember('${escapeHtml(m)}')">Quitar</button>
          </div>`;
      }).join('')
    : '<em style="color:var(--text-muted);">Sin miembros</em>';

  const sel = document.getElementById('group-member-user');
  const available = _usersData.filter(u => !members.includes(u.username) && u.username !== 'root');
  sel.innerHTML = available.length
    ? available.map(u => `<option value="${escapeHtml(u.username)}">${escapeHtml(u.full_name ? `${u.full_name} (${u.username})` : u.username)}</option>`).join('')
    : '<option value="">(sin usuarios disponibles)</option>';

  openModal('modal-group-members');
}

async function addGroupMember() {
  const sel = document.getElementById('group-member-user');
  const username = sel.value;
  if (!username) return;
  try {
    await apiFetch('/api/users/groups', {
      method: 'POST',
      body: JSON.stringify({ username, group: _activeGroup, action: 'add' }),
    });
    showToast(`${username} añadido a ${_activeGroup}.`, 'success');
    loadUsersAndGroups();
    openGroupMembers(_activeGroup);
  } catch (e) {
    // Ya mostrado
  }
}

async function removeGroupMember(username) {
  if (username === 'www-data') {
    showToast('La cuenta www-data está protegida y es requerida por el panel web.', 'warning');
    return;
  }
  showUserActionConfirm({
    title: `Quitar miembro de grupo`,
    icon: 'users',
    alertType: 'warning',
    alertText: `Se revocará la membresía del usuario [${username}] en el grupo departamental [${_activeGroup}].`,
    desc: `El usuario perderá de inmediato los permisos de acceso y herencia ACL asociados a este grupo.`,
    prompt: `¿Confirmas quitar a [${username}] del grupo [${_activeGroup}]?`,
    btnText: 'Quitar del grupo',
    btnClass: 'btn-danger',
    onConfirm: async () => {
      try {
        await apiFetch('/api/users/groups', {
          method: 'POST',
          body: JSON.stringify({ username, group: _activeGroup, action: 'remove' }),
        });
        showToast(`${username} quitado de ${_activeGroup}.`, 'success');
        loadUsersAndGroups();
        openGroupMembers(_activeGroup);
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    },
  });
}

function openRenameGroup(name) {
  document.getElementById('rename-group-old').value = name;
  document.getElementById('rename-group-new').value = '';
  openModal('modal-rename-group');
}

async function submitRenameGroup(event) {
  event.preventDefault();
  const oldName = document.getElementById('rename-group-old').value;
  const newName = document.getElementById('rename-group-new').value;

  const btn = document.getElementById('btn-submit-rename-group') || event.target.querySelector('button[type="submit"]');
  if (btn) { btn.disabled = true; btn.classList.add('is-loading'); }

  try {
    await apiFetch('/api/groups/rename', {
      method: 'POST',
      body: JSON.stringify({ old: oldName, new: newName }),
    });
    showToast(`Grupo renombrado a ${newName}.`, 'success');
    closeModal('modal-rename-group');
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) { btn.disabled = false; btn.classList.remove('is-loading'); }
  }
}

async function openNewUser() {
  try {
    const res = await apiFetch('/api/groups');
    _groupsData = res.data || [];
  } catch (e) {
    // Sin grupos disponibles
  }
  const form = document.getElementById('form-new-user');
  if (form) form.reset();
  selectRoleCard('user', 'standard');
  populateUserGroupOptions(_groupsData);
  updateEffectiveAccessPreview(false);
  openModal('modal-new-user');
}

// ==============================================================================
// Matriz de permisos de acceso (grupos × recursos y usuarios × recursos)
// ==============================================================================
let _permData = { shares: [], groups: [], users: [] };
let _permTab = 'groups';

function setPermTab(tab) {
  _permTab = tab;
  document.querySelectorAll('#perm-filter-chips .chip-btn').forEach(b => {
    b.classList.toggle('active', b.dataset.perm === tab);
  });
  renderPermMatrix();
}

async function loadAccessMatrix() {
  const container = document.getElementById('perm-matrix-container');
  if (!container) return;
  try {
    const res = await apiFetch('/api/shares/access');
    _permData = res.data || { shares: [], groups: [], users: [] };
    renderPermMatrix();
  } catch (e) {
    container.innerHTML = `<p style="padding:18px; color:var(--accent-danger);">Error al cargar la matriz de permisos: ${escapeHtml(e.message)}</p>`;
  }
}

function permBadge(level) {
  if (level === 'write') return '<span class="badge badge-ok">Lectura y escritura</span>';
  if (level === 'read') return '<span class="badge badge-blue">Solo lectura</span>';
  return '<span class="badge badge-gray">Sin acceso</span>';
}

function renderPermMatrix() {
  const container = document.getElementById('perm-matrix-container');
  if (!container) return;

  const shares = _permData.shares || [];
  const rows = _permTab === 'groups' ? (_permData.groups || []) : (_permData.users || []);
  if (!shares.length) {
    container.innerHTML = '<div style="padding:24px; text-align:center; color:var(--text-muted);"><p>No hay recursos compartidos registrados en el sistema.</p></div>';
    return;
  }
  if (!rows.length) {
    if (_permTab === 'groups') {
      container.innerHTML = `
        <div style="padding:24px; text-align:center; color:var(--text-secondary);">
          <svg class="icon icon-lg" style="color:var(--text-muted); margin-bottom:8px;"><use href="#icon-shield"></use></svg>
          <div style="font-weight:600; font-size:14px; margin-bottom:4px;">No hay grupos asignados a los recursos compartidos</div>
          <p style="font-size:12.5px; max-width:480px; margin:0 auto; color:var(--text-muted);">
            Los grupos corporativos actuales no tienen permisos específicos asignados en las carpetas compartidas registradas.
            Puedes asignar grupos directamente en la configuración de cada recurso compartido o alternar a la pestaña <strong>Usuarios × Recursos</strong>.
          </p>
        </div>
      `;
    } else {
      container.innerHTML = '<div style="padding:24px; text-align:center; color:var(--text-muted);"><p>No hay usuarios registrados con acceso a estos recursos.</p></div>';
    }
    return;
  }

  const kind = _permTab === 'groups' ? 'group' : 'user';
  let html = '<table class="nas-table"><thead><tr><th style="min-width:180px;">Entidad</th>';
  shares.forEach(s => { html += `<th>${escapeHtml(s)}</th>`; });
  html += '</tr></thead><tbody>';
  rows.forEach(r => {
    html += `<tr data-entity="${escapeHtml(r.name)}"><td><strong>${escapeHtml(r.name)}</strong>${r.is_admin ? ' <span class="badge badge-warn">Admin</span>' : ''}</td>`;
    shares.forEach(s => {
      const level = r[s] || 'none';
      html += `<td><button type="button" class="chip-btn perm-cell-btn" style="width:100%;" onclick="cyclePerm('${kind}','${escapeHtml(r.name)}','${escapeHtml(s)}', this)">${permBadge(level)}</button></td>`;
    });
    html += '</tr>';
  });
  html += '</tbody></table>';
  container.innerHTML = html;
  applyPermsSearch();
}

function applyPermsSearch() {
  const q = (document.getElementById('perms-search')?.value || '').toLowerCase().trim();
  document.querySelectorAll('#perm-matrix-container table tbody tr[data-entity]').forEach(tr => {
    const ent = (tr.dataset.entity || '').toLowerCase();
    tr.style.display = (!q || ent.includes(q)) ? '' : 'none';
  });
}

async function cyclePerm(kind, name, share, btnEl = null) {
  const order = ['none', 'read', 'write'];
  const row = (_permTab === 'groups' ? _permData.groups : _permData.users).find(r => r.name === name);
  const current = row ? (row[share] || 'none') : 'none';
  const next = order[(order.indexOf(current) + 1) % order.length];

  if (btnEl) {
    btnEl.disabled = true;
    btnEl.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/shares/access', {
      method: 'POST',
      body: JSON.stringify({ share, kind, name, level: next }),
    });
    showToast(`Permiso de ${name} en ${share}: ${next}.`, 'success');
    await loadAccessMatrix();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btnEl) {
      btnEl.disabled = false;
      btnEl.classList.remove('is-loading');
    }
  }
}

async function repairAcls(shareName = null) {
  try {
    showToast('Auditando consistencia de ACL contra smb.conf...', 'info');
    const auditUrl = '/api/shares/access/audit' + (shareName ? `?share=${encodeURIComponent(shareName)}` : '');
    const auditRes = await apiFetch(auditUrl);
    const auditData = auditRes.data || auditRes;

    const outOfSync = auditData.out_of_sync_count ?? 0;
    const total = auditData.total ?? (auditData.shares ? Object.keys(auditData.shares).length : 0);
    const sharesMap = auditData.shares || {};

    const executeRepair = async (force) => {
      try {
        const payload = { share: shareName || '', force };
        const res = await apiFetch('/api/shares/access/repair', { method: 'POST', body: JSON.stringify(payload) });
        showToast(res.message || 'ACL procesadas correctamente.', 'success');
        loadAccessMatrix();
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    };

    if (shareName) {
      const sAudit = sharesMap[shareName] || auditData;
      if (sAudit && sAudit.in_sync) {
        showUserActionConfirm({
          title: `Reaplicar ACL: [${shareName}]`,
          icon: 'shield',
          alertType: 'info',
          alertText: `El recurso [${shareName}] ya tiene sus ACL perfectamente sincronizadas con smb.conf (cero discrepancias).`,
          desc: `La reaplicación forzará la ejecución recursiva de setfacl -R en todas las carpetas y archivos dentro del recurso.`,
          prompt: `¿Deseas forzar la reaplicación recursiva de ACL de todos modos?`,
          btnText: 'Forzar reaplicación',
          btnClass: 'btn-primary',
          onConfirm: () => executeRepair(true),
        });
      } else {
        const discList = ((sAudit && sAudit.discrepancies) || []).map(d => `• ${d}`).join('<br>');
        showUserActionConfirm({
          title: `Reparar ACL: [${shareName}]`,
          icon: 'alert-triangle',
          alertType: 'warning',
          alertText: `Se detectaron desajustes entre smb.conf y las ACL del sistema en [${shareName}]:<br><div style="margin-top:6px; font-family:monospace; font-size:11.5px;">${discList || 'Discrepancias en permisos'}</div>`,
          desc: `Se reajustarán las ACL POSIX y las máscaras efectivas para que coincidan con la definición del recurso.`,
          prompt: `¿Proceder con la reparación automática de ACL?`,
          btnText: 'Reparar ACL',
          btnClass: 'btn-primary',
          onConfirm: () => executeRepair(false),
        });
      }
    } else {
      if (outOfSync === 0) {
        showUserActionConfirm({
          title: `Reaplicar ACL en todos los recursos`,
          icon: 'shield',
          alertType: 'info',
          alertText: `Todas las ACL de los ${total} recurso(s) están perfectamente sincronizadas con smb.conf.`,
          desc: `Se ejecutará una reaplicación recursiva total de permisos setfacl sobre todos los recursos compartidos.`,
          prompt: `¿Deseas forzar la reaplicación recursiva en todos los recursos?`,
          btnText: 'Forzar reaplicación total',
          btnClass: 'btn-primary',
          onConfirm: () => executeRepair(true),
        });
      } else {
        const outList = Object.entries(sharesMap)
          .filter(([_, v]) => !v.in_sync)
          .map(([k, v]) => `• <strong>${k}</strong>: ${(v.discrepancies || []).length} discrepancia(s)`)
          .join('<br>');
        showUserActionConfirm({
          title: `Reparación de ACL del sistema`,
          icon: 'alert-triangle',
          alertType: 'warning',
          alertText: `Auditoría: ${outOfSync} de ${total} recurso(s) requieren sincronización:<br><div style="margin-top:6px; font-size:12px;">${outList}</div>`,
          desc: `La reparación inteligente procesará únicamente las carpetas compartidas con desajustes detectados.`,
          prompt: `¿Proceder con la reparación inteligente de ACL?`,
          btnText: 'Reparar desajustes',
          btnClass: 'btn-primary',
          onConfirm: () => executeRepair(false),
        });
      }
    }
  } catch (e) {
    // Ya mostrado por apiFetch
  }
}

async function submitNewUser(event) {
  event.preventDefault();
  const username = document.getElementById('user-uname').value;
  const full_name = document.getElementById('user-fullname').value;
  const password = document.getElementById('user-pass').value;
  const role = document.getElementById('user-role-val')?.value || 'standard';
  const isAdmin = role === 'admin' || role === 'superadmin';
  const canWeb = role === 'operator' || isAdmin;
  const isSuperadmin = role === 'superadmin';
  const sambaEnabled = document.getElementById('user-samba-enabled').checked;

  const selectedGroups = Array.from(document.querySelectorAll('input[name="user_group"]:checked'))
    .map(cb => cb.value);

  const btn = document.getElementById('btn-submit-new-user') || event.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/users', {
      method: 'POST',
      body: JSON.stringify({
        username,
        full_name,
        password,
        groups: selectedGroups,
        is_admin: isAdmin,
        can_web: canWeb,
        samba_enabled: sambaEnabled,
        is_superadmin: isSuperadmin,
      }),
    });

    showToast(`Usuario [${username}] creado y sincronizado en Samba.`, 'success');

    // Guardar datos para ficha de acceso y bienvenida
    const serverHost = window.location.hostname || 'SRV-NAS';
    const roleTitle = isSuperadmin ? 'Superadministrador' : (isAdmin ? 'Administrador' : (canWeb ? 'Operador Web' : 'Empleado de Red'));
    _lastCreatedUserMemo = {
      username,
      full_name: full_name || '',
      password,
      roleTitle,
      serverHost,
      unc: `\\\\${serverHost}`,
    };

    closeModal('modal-new-user');
    document.getElementById('form-new-user').reset();

    // Mostrar modal de bienvenida con ruta UNC
    const uncInput = document.getElementById('welcome-unc-path');
    if (uncInput) {
      uncInput.value = `\\\\${serverHost}`;
    }
    const infoEl = document.getElementById('welcome-user-info');
    if (infoEl) {
      const roleBadge = isSuperadmin
        ? '<span class="badge badge-err">Superadministrador</span>'
        : (isAdmin
          ? '<span class="badge badge-warn">Administrador</span>'
          : (canWeb ? '<span class="badge badge-blue">Operador Web</span>' : '<span class="badge badge-gray">Empleado de Red</span>'));
      infoEl.innerHTML = `
        <div style="margin-bottom:4px;"><strong>Usuario:</strong> ${escapeHtml(username)}</div>
        <div style="margin-bottom:4px;"><strong>Nombre:</strong> ${escapeHtml(full_name || 'Sin nombre asignado')}</div>
        <div style="margin-bottom:4px;"><strong>Rol asignado:</strong> ${roleBadge}</div>
        <div><strong>Contraseña temporal:</strong> <code style="user-select:all;">${escapeHtml(password)}</code></div>
      `;
    }
    openModal('modal-user-welcome');

    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

function copyUserWelcomeMemo() {
  if (!_lastCreatedUserMemo) {
    showToast('No hay ficha de usuario disponible.', 'warning');
    return;
  }
  const memo = [
    `=== FICHA DE ACCESO A RECURSOS (SRV-NAS) ===`,
    `Servidor / Host: ${_lastCreatedUserMemo.serverHost}`,
    `Ruta de Red Windows (UNC): ${_lastCreatedUserMemo.unc}`,
    `Usuario de Acceso: ${_lastCreatedUserMemo.username}`,
    `Nombre / Cargo: ${_lastCreatedUserMemo.full_name || 'N/A'}`,
    `Rol asignado: ${_lastCreatedUserMemo.roleTitle}`,
    `Contraseña temporal: ${_lastCreatedUserMemo.password || '(asignada en el alta)'}`,
    `============================================`,
  ].join('\n');

  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(memo).then(() => {
      showToast('Ficha de acceso copiada al portapapeles.', 'success');
    }).catch(() => {
      copyUncPath(memo);
    });
  } else {
    copyUncPath(memo);
  }
}

async function deleteUser(username) {
  showUserActionConfirm({
    title: `Eliminar usuario [${username}]`,
    icon: 'trash',
    alertType: 'danger',
    alertText: `Esta acción eliminará de forma permanente la cuenta [${username}] de Linux, Samba y su directorio personal (/home/${username}).`,
    desc: `Los datos en carpetas compartidas se preservarán, pero el usuario no podrá volver a autenticarse.`,
    prompt: `¿Confirmas que deseas eliminar irreversiblemente al usuario [${username}]?`,
    btnText: 'Eliminar usuario',
    btnClass: 'btn-danger',
    onConfirm: async () => {
      try {
        await apiFetch('/api/users/delete', {
          method: 'POST',
          body: JSON.stringify({ username }),
        });
        showToast(`Usuario [${username}] eliminado.`, 'success');
        loadUsersAndGroups();
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    },
  });
}

function updateGroupPreview(input) {
  const preview = document.getElementById('group-name-preview');
  if (!preview) return;
  let val = (input ? input.value : '').trim().toLowerCase();
  while (val.startsWith('grp_')) {
    val = val.slice(4);
  }
  if (input && input.value !== val) {
    input.value = val;
  }
  preview.textContent = val ? `grp_${val}` : 'grp_...';
}

async function submitNewGroup(event) {
  event.preventDefault();
  const input = document.getElementById('group-name');
  let raw = (input ? input.value : '').trim().toLowerCase();
  while (raw.startsWith('grp_')) {
    raw = raw.slice(4);
  }
  if (!raw) {
    showToast('Por favor introduce un nombre para el grupo.', 'warning');
    return;
  }
  const fullName = `grp_${raw}`;

  const btn = document.getElementById('btn-submit-new-group') || event.target.querySelector('button[type="submit"]');
  if (btn) {
    btn.disabled = true;
    btn.classList.add('is-loading');
  }

  try {
    await apiFetch('/api/groups', {
      method: 'POST',
      body: JSON.stringify({ name: fullName }),
    });

    showToast(`Grupo [${fullName}] creado correctamente.`, 'success');
    closeModal('modal-new-group');
    const form = document.getElementById('form-new-group');
    if (form) form.reset();
    const preview = document.getElementById('group-name-preview');
    if (preview) preview.textContent = 'grp_...';
    loadUsersAndGroups();
  } catch (e) {
    // Ya mostrado
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove('is-loading');
    }
  }
}

async function deleteGroup(name) {
  showUserActionConfirm({
    title: `Eliminar grupo [${name}]`,
    icon: 'trash',
    alertType: 'danger',
    alertText: `El grupo corporativo [${name}] será eliminado del sistema operativo y de la configuración de carpetas compartidas.`,
    desc: `Los usuarios mantendrán sus cuentas, pero perderán los permisos conferidos por este grupo en Samba y el sistema de archivos.`,
    prompt: `¿Confirmas que deseas eliminar el grupo [${name}]?`,
    btnText: 'Eliminar grupo',
    btnClass: 'btn-danger',
    onConfirm: async () => {
      try {
        await apiFetch('/api/groups/delete', {
          method: 'POST',
          body: JSON.stringify({ name }),
        });
        showToast(`Grupo [${name}] eliminado.`, 'success');
        loadUsersAndGroups();
      } catch (e) {
        // Ya mostrado por apiFetch
      }
    },
  });
}

// ==============================================================================
