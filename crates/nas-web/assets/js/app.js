/**
 * NAS DEBIAN • FRONTEND JS (Vanilla ES6+)
 * Comunicación asíncrona mediante Fetch con endpoints JSON de la API MVC.
 * 100% Offline • Cero dependencias externas.
 */

'use strict';

// ==============================================================================
// 1. Estado Global y Utilidades UI
// ==============================================================================
const AppState = {
  activeView: 'dashboard',
  refreshInterval: null,
  groupsCache: [],
  logSource: 'panel',
  logSearchTimer: null,
  logsCache: [],
  files: {
    root: 'nas',
    currentPath: '',
    items: [],
    displayItems: [],
    filterQuery: '',
    sortKey: 'name_asc',
    viewMode: localStorage.getItem('nas_file_view') || 'grid',
    inTrash: false,
    trashItems: [],
    trashFilterQuery: '',
    selectedItemPath: null,
    currentPreviewFile: null,
    currentPreviewText: '',
    isEditingPreview: false,
  },
  terminal: {
    cwd: '/srv/nas',
    history: [],
    historyIndex: -1,
    isExecuting: false,
    ptyStarted: false,
    ptyTimer: null,
  },
  domain: {
    status: null,
  },
};





// ==============================================================================
// 2. Navegación entre Vistas
// ==============================================================================
const VALID_VIEWS = [
  'dashboard', 'files', 'logs', 'storage', 'networking', 'services', 'terminal',
  'shares', 'backups', 'users', 'diagnostics', 'updates', 'applications', 'domain'
];

// Módulos permitidos al rol operador (grp_web)
const WEB_USER_VIEWS = ['dashboard', 'files', 'logs', 'shares', 'backups', 'users', 'services', 'diagnostics', 'networking'];

// Ajusta la interfaz según el rol de la sesión (administrador / superadministrador / operador).
function applyRoleUi() {
  const isAdmin = window.NAS_IS_ADMIN !== false;
  const isSuper = window.NAS_IS_SUPER === true;

  ['user-admin-role-wrap', 'edit-user-admin-role-wrap'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.style.display = isAdmin ? '' : 'none';
  });
  ['user-superadmin-role-wrap', 'edit-user-superadmin-role-wrap'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.style.display = isSuper ? '' : 'none';
  });

  // Ocultar módulo de respaldos en frontend si el rol exclusivo es ARCHIVOS
  if (window.NAS_SERVER_ROLE === 'ARCHIVOS') {
    document.querySelectorAll('[data-view="backups"]').forEach(el => el.style.display = 'none');
    const backupView = document.getElementById('view-backups');
    if (backupView) backupView.style.display = 'none';
  }
}

function switchView(viewName, updateHash = true) {
  if (viewName === 'permissions') {
    viewName = 'users';
  }
  if (viewName === 'diagnostics') {
    viewName = 'services';
  }
  if (viewName === 'updates') {
    viewName = 'applications';
  }
  if (!VALID_VIEWS.includes(viewName)) {
    viewName = 'dashboard';
  }

  // Si el servidor opera en rol ARCHIVOS, denegar acceso a backups
  if (window.NAS_SERVER_ROLE === 'ARCHIVOS' && viewName === 'backups') {
    viewName = 'dashboard';
  }

  // Los operadores solo pueden ver los módulos delegados; el resto se redirige al dashboard.
  if (window.NAS_IS_ADMIN === false && !WEB_USER_VIEWS.includes(viewName)) {
    viewName = 'dashboard';
  }

  // La terminal web está confinada exclusivamente al superadministrador.
  if (viewName === 'terminal' && window.NAS_IS_SUPER !== true) {
    viewName = 'dashboard';
  }

  AppState.activeView = viewName;

  if (updateHash && window.location.hash !== '#' + viewName) {
    window.location.hash = '#' + viewName;
  }

  // Actualizar sidebar nav (soporta alias unificados)
  document.querySelectorAll('.sidebar-nav .nav-item').forEach(item => {
    const itemTarget = item.getAttribute('data-view');
    const isMatch = itemTarget === viewName ||
      (viewName === 'services' && itemTarget === 'diagnostics') ||
      (viewName === 'diagnostics' && itemTarget === 'services') ||
      (viewName === 'applications' && itemTarget === 'updates') ||
      (viewName === 'updates' && itemTarget === 'applications');
    if (isMatch) {
      item.classList.add('active');
    } else {
      item.classList.remove('active');
    }
  });

  // Actualizar secciones de vista (soporta alias unificados)
  document.querySelectorAll('.view-section').forEach(sec => {
    const isSecMatch = sec.id === `view-${viewName}` ||
      (viewName === 'services' && sec.id === 'view-diagnostics') ||
      (viewName === 'diagnostics' && sec.id === 'view-services') ||
      (viewName === 'applications' && sec.id === 'view-updates') ||
      (viewName === 'updates' && sec.id === 'view-applications');
    if (isSecMatch) {
      sec.classList.add('active');
    } else {
      sec.classList.remove('active');
    }
  });

  // Cerrar menú móvil si estuviera abierto
  const sidebar = document.querySelector('.sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');
  if (sidebar && sidebar.classList.contains('open')) {
    sidebar.classList.remove('open');
    if (backdrop) backdrop.classList.remove('active');
  }

  // Carga de datos según la vista
  if (viewName !== 'terminal') {
    stopTerminalPolling();
  }
  switch (viewName) {
    case 'dashboard':
      refreshDashboardMetrics();
      break;
    case 'files':
      loadFiles();
      updateTrashBadge();
      break;
    case 'shares':
      loadShares();
      break;
    case 'backups':
      loadBackups();
      break;
    case 'storage':
      loadStorage();
      break;
    case 'users':
      loadUsersAndGroups();
      loadAccessMatrix();
      break;
    case 'services':
    case 'diagnostics':
      loadServices();
      loadDiagnostics();
      break;
    case 'logs':
      loadLogs();
      break;
    case 'networking':
      loadNetworking();
      break;
    case 'terminal':
      loadTerminal();
      break;
    case 'applications':
    case 'updates':
      loadUpdates();
      loadApplications();
      break;
    case 'domain':
      loadDomainStatus();
      break;
  }
}

// ==============================================================================
// 3. Clientes API Asíncronos (Fetch) & Autenticación
// ==============================================================================

// ==============================================================================
// ==============================================================================
// 15. Inicialización Robusta al Cargar el DOM
// ==============================================================================
function initApp() {
  if (window.NAS_INITIAL_SERVER_TIME) {
    initServerClock(window.NAS_INITIAL_SERVER_TIME);
  }
  applyRoleUi();

  // Configurar listeners de navegación sidebar
  document.querySelectorAll('.sidebar-nav .nav-item').forEach(item => {
    item.addEventListener('click', () => {
      const view = item.getAttribute('data-view');
      if (view) switchView(view);
      const sidebar = document.querySelector('.sidebar');
      const backdrop = document.getElementById('sidebar-backdrop');
      if (sidebar && backdrop) {
        sidebar.classList.remove('open');
        backdrop.classList.remove('active');
      }
    });
  });

  // Toggle de menú móvil
  const mobileBtn = document.getElementById('mobile-menu-btn');
  const sidebar = document.querySelector('.sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');
  if (mobileBtn && sidebar && backdrop) {
    mobileBtn.addEventListener('click', () => {
      sidebar.classList.toggle('open');
      backdrop.classList.toggle('active');
    });
    backdrop.addEventListener('click', () => {
      sidebar.classList.remove('open');
      backdrop.classList.remove('active');
    });
  }

  // Listener de cambios en el hash de la URL (evitar re-ejecución si la vista ya está activa)
  window.addEventListener('hashchange', () => {
    const hash = window.location.hash.slice(1);
    if (hash && VALID_VIEWS.includes(hash)) {
      if (AppState.activeView !== hash) {
        switchView(hash, false);
      }
    }
  });

  // Cerrar modales al hacer clic fuera del diálogo (excepto modales con formularios o .modal-static)
  document.querySelectorAll('.modal-backdrop, .modal-overlay').forEach(modal => {
    modal.addEventListener('click', (e) => {
      if (e.target === modal) {
        const hasForm = modal.querySelector('form') !== null;
        const isStatic = modal.classList.contains('modal-static') || hasForm;
        if (isStatic) {
          const dialog = modal.querySelector('.modal-dialog, .modal-dialog-preview');
          if (dialog) {
            dialog.classList.remove('modal-pulse');
            void dialog.offsetWidth;
            dialog.classList.add('modal-pulse');
            setTimeout(() => {
              dialog.classList.remove('modal-pulse');
            }, 250);
          }
          return;
        }
        closeModal(modal.id);
      }
    });
  });

  function isFilesDragEvent(e) {
    if (!e.dataTransfer || !e.dataTransfer.types) return false;
    const types = e.dataTransfer.types;
    return types.includes ? types.includes('Files') : Array.from(types).includes('Files');
  }

  // Cerrar modal y previsualizador de forma inmediata al presionar Escape (excepto estáticos/formularios)
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      document.querySelectorAll('.modal-backdrop.active, .modal-backdrop.open, .modal-overlay.active, .modal-overlay.open').forEach(modal => {
        const hasForm = modal.querySelector('form') !== null;
        const isStatic = modal.classList.contains('modal-static') || hasForm;
        if (isStatic) {
          const dialog = modal.querySelector('.modal-dialog, .modal-dialog-preview');
          if (dialog) {
            dialog.classList.remove('modal-pulse');
            void dialog.offsetWidth;
            dialog.classList.add('modal-pulse');
            setTimeout(() => {
              dialog.classList.remove('modal-pulse');
            }, 250);
          }
          return;
        }
        closeModal(modal.id);
      });
      const dz = document.getElementById('file-dropzone-container');
      if (dz) dz.classList.remove('drag-active');
    }
  });

  // Inicializar componentes interactivos
  initTerminal();
  populateShareGroupOptions();

  // Configurar listeners de la dropzone reactiva para Explorador de Archivos
  const dropzone = document.getElementById('file-dropzone-container');
  if (dropzone) {
    let dragCounter = 0;
    dropzone.addEventListener('dragenter', (e) => {
      if (!isFilesDragEvent(e)) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounter++;
      dropzone.classList.add('drag-active');
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    }, false);

    dropzone.addEventListener('dragover', (e) => {
      if (!isFilesDragEvent(e)) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
      if (!dropzone.classList.contains('drag-active')) {
        dropzone.classList.add('drag-active');
      }
    }, false);

    dropzone.addEventListener('dragleave', (e) => {
      if (!isFilesDragEvent(e)) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounter--;
      if (dragCounter <= 0) {
        dragCounter = 0;
        dropzone.classList.remove('drag-active');
      }
    }, false);

    dropzone.addEventListener('drop', (e) => {
      if (!isFilesDragEvent(e)) return;
      e.preventDefault();
      e.stopPropagation();
      dragCounter = 0;
      dropzone.classList.remove('drag-active');
      const dt = e.dataTransfer;
      const files = dt ? dt.files : null;
      if (files && files.length > 0) {
        uploadFiles(files);
      }
    }, false);
  }

  // Deseleccionar al hacer clic en fondo libre de la cuadricula
  const gridContainer = document.getElementById('files-grid-container');
  if (gridContainer) {
    gridContainer.addEventListener('click', (e) => {
      if (e.target === gridContainer) {
        document.querySelectorAll('.file-card.selected').forEach(c => c.classList.remove('selected'));
        AppState.files.selectedItemPath = null;
      }
    });
  }

  // Prevenir navegación accidental del navegador al arrastrar fuera de la dropzone
  window.addEventListener('dragover', (e) => {
    if (AppState.activeView === 'files') {
      e.preventDefault();
    }
  }, false);
  window.addEventListener('drop', (e) => {
    if (AppState.activeView === 'files') {
      e.preventDefault();
    }
  }, false);

  // Inicializar estado del explorador de archivos y papelera
  setFileViewMode(AppState.files.viewMode, false);
  updateTrashBadge();
  loadStorageUnits();
  initSystemNotifications();
  initFilesKeyboardShortcuts();

  // Cerrar drawer de notificaciones al hacer clic fuera
  document.addEventListener('click', (e) => {
    const drawer = document.getElementById('notifications-drawer');
    const btn = document.getElementById('btn-notifications');
    if (drawer && drawer.style.display !== 'none' && !drawer.contains(e.target) && !btn?.contains(e.target)) {
      drawer.style.display = 'none';
    }
  });

  // Determinar vista inicial
  const hash = window.location.hash ? window.location.hash.slice(1) : '';
  const initialView = (hash && VALID_VIEWS.includes(hash))
    ? hash
    : (window.SERVER_ACTIVE_VIEW && VALID_VIEWS.includes(window.SERVER_ACTIVE_VIEW)
      ? window.SERVER_ACTIVE_VIEW
      : 'dashboard');

  switchView(initialView, false);

  // Polling ligero de métricas cada 30 segundos
  AppState.refreshInterval = setInterval(() => {
    if (AppState.activeView === 'dashboard') {
      refreshDashboardMetrics();
    }
  }, 30000);
}

// ==============================================================================
// 15. Notificaciones y Alertas Persistentes del Sistema
// ==============================================================================
let _systemNotifications = [];

function initSystemNotifications() {
  try {
    const saved = localStorage.getItem('nas_system_notifications');
    if (saved) {
      _systemNotifications = JSON.parse(saved);
    }
  } catch (e) { }
  renderSystemNotifications();
}

function toggleNotificationsDrawer() {
  const drawer = document.getElementById('notifications-drawer');
  if (!drawer) return;
  const isHidden = (drawer.style.display === 'none' || !drawer.style.display);
  drawer.style.display = isHidden ? 'block' : 'none';
}

function clearNotifications() {
  _systemNotifications = [];
  try {
    localStorage.removeItem('nas_system_notifications');
  } catch (e) { }
  renderSystemNotifications();
}

function addSystemNotification(item) {
  if (!item.id) item.id = 'notif_' + Date.now() + '_' + Math.random().toString(36).substr(2, 5);
  if (!item.time) item.time = new Date().toLocaleTimeString();

  const idx = _systemNotifications.findIndex(x => x.id === item.id);
  if (idx >= 0) {
    _systemNotifications[idx] = item;
  } else {
    _systemNotifications.unshift(item);
  }
  if (_systemNotifications.length > 25) {
    _systemNotifications = _systemNotifications.slice(0, 25);
  }
  try {
    localStorage.setItem('nas_system_notifications', JSON.stringify(_systemNotifications));
  } catch (e) { }
  renderSystemNotifications();
}

function renderSystemNotifications() {
  const badge = document.getElementById('notifications-badge');
  const list = document.getElementById('notifications-list');
  const count = _systemNotifications.length;

  if (badge) {
    badge.textContent = count;
    badge.style.display = count > 0 ? 'inline-block' : 'none';
  }

  if (list) {
    if (count === 0) {
      list.innerHTML = '<div style="text-align:center; padding:16px 0;">No hay notificaciones pendientes</div>';
    } else {
      list.innerHTML = _systemNotifications.map(n => {
        const typeColor = n.type === 'danger' ? 'var(--accent-danger)' : (n.type === 'warning' ? 'var(--accent-warning)' : 'var(--accent-primary)');
        return `
          <div style="border-left:3px solid ${typeColor}; padding:6px 8px; background:rgba(255,255,255,0.03); border-radius:4px;">
            <div style="display:flex; justify-content:space-between; align-items:center;">
              <strong style="color:var(--text-primary); font-size:12px;">${escapeHtml(n.title)}</strong>
              <span style="font-size:10px; color:var(--text-muted);">${escapeHtml(n.time)}</span>
            </div>
            <div style="font-size:11px; margin-top:3px; color:var(--text-secondary);">${escapeHtml(n.message)}</div>
          </div>
        `;
      }).join('');
    }
  }

  renderSystemBanners();
}

function renderSystemBanners() {
  const container = document.getElementById('system-banners');
  if (!container) return;

  const urgent = _systemNotifications.filter(n => n.sticky || n.type === 'danger' || n.type === 'warning');
  if (urgent.length === 0) {
    container.innerHTML = '';
    container.style.display = 'none';
    return;
  }

  container.style.display = 'flex';
  container.style.flexDirection = 'column';
  container.style.gap = '8px';
  container.innerHTML = urgent.map(n => {
    const isDanger = n.type === 'danger';
    const boxClass = isDanger ? 'alert-box-danger' : 'alert-box-warning';
    const iconHref = isDanger ? '#icon-x-circle' : '#icon-alert-triangle';
    return `
      <div class="alert-box ${boxClass}" style="display:flex; justify-content:space-between; align-items:center; margin:0;">
        <div style="display:flex; align-items:center; gap:8px;">
          <svg class="icon icon-sm" style="flex-shrink:0;"><use href="${iconHref}"></use></svg>
          <div>
            <strong>${escapeHtml(n.title)}:</strong> <span>${escapeHtml(n.message)}</span>
          </div>
        </div>
        <button type="button" class="btn btn-secondary btn-xs" onclick="dismissBanner('${escapeHtml(n.id)}')" style="padding:2px 8px; font-size:11px;">Entendido</button>
      </div>
    `;
  }).join('');
}

function dismissBanner(id) {
  _systemNotifications = _systemNotifications.filter(n => n.id !== id);
  try {
    localStorage.setItem('nas_system_notifications', JSON.stringify(_systemNotifications));
  } catch (e) { }
  renderSystemNotifications();
}

function toggleSidebar() {
  const sidebar = document.querySelector('.sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');
  if (sidebar) sidebar.classList.toggle('open');
  if (backdrop) backdrop.classList.toggle('active');
}

// ==============================================================================
// 16. Aliases Globales para Compatibilidad y Eventos Onclick
// ==============================================================================
// Cabecera y Navegación
window.switchView = switchView;
window.toggleSidebar = toggleSidebar;
window.toggleNotificationsDrawer = toggleNotificationsDrawer;
window.toggleNotificationDrawer = toggleNotificationsDrawer;
window.clearNotifications = clearNotifications;
window.emptyNotifications = clearNotifications;
window.markAllAsRead = clearNotifications;
window.logout = logoutSession;
window.logoutSession = logoutSession;
window.refreshDashboardMetrics = refreshDashboardMetrics;
window.toggleTheme = toggleTheme;

// Almacenamiento
window.openStorageManage = openStorageManage;
window.openCreatePoolWizard = openCreatePoolWizard;
window.createPoolNext = createPoolNext;
window.updateCreatePoolCapacity = updateCreatePoolCapacity;
window.submitCreatePool = submitCreatePool;
window.storageWizardNext = storageWizardNext;
window.openFormatDisk = openFormatDisk;
window.runStorageScrub = runStorageScrub;
window.confirmAuditBtrfs = confirmAuditBtrfs;
window.runStorageTrim = runStorageTrim;
window.confirmTrimSsd = confirmTrimSsd;
window.executeStorageUnmountAndProceed = executeStorageUnmountAndProceed;
window.openCreateSubvolumeModal = openCreateSubvolumeModal;
window.loadStorageSubvolumes = loadStorageSubvolumes;
window.renderStorageOverview = renderStorageOverview;
window.loadStorage = loadStorage;
window.loadStorageUnits = loadStorageUnits;
window.renderStoragePools = renderStoragePools;
window.submitStorageManage = submitStorageManage;
window.toggleStorageOp = toggleStorageOp;
window.onStorageDeviceChange = onStorageDeviceChange;

// Recursos Compartidos (Samba)
window.openNewShare = openNewShare;
window.openNewShareModal = openNewShare;
window.openEditShare = openEditShare;
window.openSharePermissions = openSharePermissions;
window.deleteShare = deleteShare;
window.copyUncPath = copyUncPath;
window.copyShareUnc = copyShareUnc;
window.repairSambaPermissions = repairSambaPermissions;
window.repairAcls = repairAcls;
window.submitNewShare = submitNewShare;
window.submitEditShare = submitEditShare;
window.toggleSchemeFields = toggleSchemeFields;
window.toggleEditSchemeFields = toggleEditSchemeFields;

// Central de Respaldos (Backups)
window.openNewBackupModal = () => openModal('modal-new-backup');
window.openNewBackup = () => openModal('modal-new-backup');
window.openEditBackup = openEditBackup;
window.editBackup = openEditBackup;
window.runBackupNow = runBackupTask;
window.runBackupTask = runBackupTask;
window.openRunBackupModal = openRunBackupModal;
window.executeRunBackupConfirm = executeRunBackupConfirm;
window.stopBackup = stopBackupTask;
window.stopBackupTask = stopBackupTask;
window.openStopBackupModal = openStopBackupModal;
window.executeStopBackupConfirm = executeStopBackupConfirm;
window.viewBackupLog = viewBackupLogs;
window.viewBackupLogs = viewBackupLogs;
window.deleteBackup = deleteBackupTask;
window.deleteBackupTask = deleteBackupTask;
window.testBackupConnection = testBackupConnection;
window.testEditBackupConnection = testEditBackupConnection;
window.submitNewBackup = submitNewBackup;
window.submitEditBackup = submitEditBackup;
window.toggleBackupFields = toggleBackupFields;
window.toggleEditBackupFields = toggleEditBackupFields;
window.switchBackupTab = switchBackupTab;
window.loadPermanentBackups = loadPermanentBackups;
window.openFreezeModal = openFreezeModal;
window.submitFreezeBackup = submitFreezeBackup;
window.openRestoreBackup = openRestoreBackup;
window.toggleRestoreTargetField = toggleRestoreTargetField;
window.submitRestoreBackup = submitRestoreBackup;
window.deletePermanentBackup = deletePermanentBackup;

// Explorador de Archivos y Papelera
window.filterCurrentFiles = filterCurrentFiles;
window.setFilesSort = setFilesSort;
window.toggleTableSort = toggleTableSort;
window.copyCurrentUncPath = copyCurrentUncPath;
window.copyItemUncPath = copyItemUncPath;
window.openNewFileModal = openNewFileModal;
window.openNewFile = openNewFileModal;
window.submitNewFile = submitNewFile;
window.filterTrashItems = filterTrashItems;
window.openRestoreAllTrashModal = openRestoreAllTrashModal;
window.confirmRestoreAllTrash = confirmRestoreAllTrash;
window.loadFiles = loadFiles;
window.updateFilesStatusBar = updateFilesStatusBar;
window.triggerFileUpload = triggerFileInput;
window.triggerFileInput = triggerFileInput;
window.openNewFolderModal = openNewFolderModal;
window.openNewFolder = openNewFolderModal;
window.downloadFolderZip = downloadFolderZip;
window.downloadCurrentFolderZip = downloadCurrentFolderZip;
window.openEmptyTrashModal = openEmptyTrashModal;
window.emptyTrash = openEmptyTrashModal;
window.confirmEmptyTrash = confirmEmptyTrash;
window.toggleTrashView = toggleTrashView;
window.exitTrashView = exitTrashView;
window.restoreTrashItem = restoreTrashItem;
window.deleteTrashItem = deleteTrashItem;
window.handleTrashAction = handleTrashAction;
window.refreshCurrentFileView = refreshCurrentFileView;
window.setFileViewMode = setFileViewMode;
window.changeFilesRoot = changeFilesRoot;
window.navigateToSubpath = navigateToSubpath;
window.downloadFile = downloadFile;
window.openRenameModal = openRenameModal;
window.submitRenameFile = submitRenameFile;
window.openMoveModal = openMoveModal;
window.submitMoveFile = submitMoveFile;
window.openDeleteModal = openDeleteModal;
window.confirmDeleteFile = confirmDeleteFile;
window.previewFile = previewFile;
window.openFilePreview = previewFile;
window.copyPreviewContent = copyPreviewContent;
window.togglePreviewEditMode = togglePreviewEditMode;
window.saveCurrentPreviewFile = saveCurrentPreviewFile;
window.downloadPreviewFile = downloadPreviewFile;
window.navigatePptxSlide = navigatePptxSlide;

// Usuarios y Grupos
window.switchUsersTab = switchUsersTab;
window.executeUserActionConfirm = executeUserActionConfirm;
window.updateUsersKpis = updateUsersKpis;
window.updateUserChipCounters = updateUserChipCounters;
window.applyGroupsFilters = applyGroupsFilters;
window.applyPermsSearch = applyPermsSearch;
window.sortUsersTable = sortUsersTable;
window.renderUsersTableBody = renderUsersTableBody;
window.copyPasswordInput = copyPasswordInput;
window.copyUserWelcomeMemo = copyUserWelcomeMemo;
window.filterGroupMembers = filterGroupMembers;
window.repairAcls = repairAcls;
window.openNewUser = openNewUser;
window.openNewUserModal = openNewUser;
window.openNewGroup = () => {
  const p = document.getElementById('group-name-preview');
  if (p) p.textContent = 'grp_...';
  openModal('modal-new-group');
};
window.openNewGroupModal = () => {
  const p = document.getElementById('group-name-preview');
  if (p) p.textContent = 'grp_...';
  openModal('modal-new-group');
};
window.repairUserAcls = repairAcls;
window.openEditUser = openEditUser;
window.toggleUser = toggleUser;
window.deleteUser = deleteUser;
window.openGroupMembers = openGroupMembers;
window.openGroupMembersModal = openGroupMembers;
window.openRenameGroup = openRenameGroup;
window.deleteGroup = deleteGroup;
window.generateAndSetPassword = generateAndSetPassword;
window.togglePasswordVisibility = togglePasswordVisibility;
window.setUsersFilter = setUsersFilter;
window.applyUsersFilters = applyUsersFilters;
window.submitNewUser = submitNewUser;
window.submitUpdateUser = submitUpdateUser;
window.submitEditUser = submitUpdateUser;
window.updateGroupPreview = updateGroupPreview;
window.submitNewGroup = submitNewGroup;
window.submitRenameGroup = submitRenameGroup;
window.addGroupMember = addGroupMember;
window.submitAddGroupMember = addGroupMember;
window.removeGroupMember = removeGroupMember;
window.setPermTab = setPermTab;
window.loadAccessMatrix = loadAccessMatrix;
window.cyclePerm = cyclePerm;
window.toggleAccessMatrixCell = cyclePerm;

// Servicios y Diagnóstico
window.loadServices = loadServices;
window.loadDiagnostics = loadDiagnostics;
window.startService = startService;
window.stopService = stopService;
window.reloadService = reloadService;
window.restartService = restartService;
window.manageServiceAction = manageServiceAction;
window.runDiagnosticTest = runDiagnosticTest;

// Aplicaciones y Actualizaciones
window.loadUpdates = loadUpdates;
window.checkSystemUpdates = loadUpdates;
window.applySystemUpdate = applySystemUpdate;
window.triggerSystemUpdate = applySystemUpdate;
window.openSystemUpdateModal = openSystemUpdateModal;
window.startSystemUpdate = startSystemUpdate;
window.loadApplications = loadApplications;
window.switchApplicationsTab = switchApplicationsTab;
window.triggerSystemUpdatesCheck = triggerSystemUpdatesCheck;
window.renderSambaSessionsTable = renderSambaSessionsTable;
window.openSambaSessionsModal = openSambaSessionsModal;
window.reloadSambaService = reloadSambaService;
window.reloadPhpService = reloadPhpService;
window.restartWsddService = restartWsddService;
window.triggerBtrfsScrub = triggerBtrfsScrub;

// Auditoría y Registros
window.loadLogs = loadLogs;
window.setLogSource = setLogSource;
window.debounceLogSearch = debounceLogSearch;
window.exportOrCopyLogs = exportOrCopyLogs;
window.verifyAuditIntegrity = verifyAuditIntegrity;

// Consola Terminal
window.loadTerminal = loadTerminal;
window.clearTerminal = clearTerminal;
window.reconnectTerminal = reconnectTerminal;
window.resizeTerminal = resizeTerminal;
window.sendTerminalKey = sendTerminalKey;
window.runQuickCommand = runQuickCommand;

// Dominio Active Directory
window.loadDomainStatus = loadDomainStatus;
window.handleDomainDiscover = handleDomainDiscover;
window.testDomainConnection = handleDomainDiscover;
window.handleDomainJoin = handleDomainJoin;
window.joinDomain = handleDomainJoin;
window.handleDomainLeave = handleDomainLeave;
window.leaveDomain = handleDomainLeave;

// Utilidades y Notificaciones
window.cleanErrorMessage = cleanErrorMessage;
window.showToast = showToast;

// Modales Generales
window.openModal = openModal;
window.closeModal = closeModal;
window.confirmRebootServer = confirmRebootServer;

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', initApp);
} else {
  initApp();
}
