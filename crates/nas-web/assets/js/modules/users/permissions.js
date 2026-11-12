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
