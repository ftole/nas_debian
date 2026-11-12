
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

