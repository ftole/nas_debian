/**
 * ==============================================================================
 * NAS DEBIAN • LÓGICA DE INTERACTIVIDAD (VANILLA JAVASCRIPT)
 * Prototipo Web Fiel a Red Hat Cockpit / PatternFly 4 (Debian 13)
 * Organización: TEAM-JOFRATO
 * 100% Offline • Cero dependencias externas • 13 Módulos de Administración
 * ==============================================================================
 */

// Estado Global de la Aplicación
const AppState = {
  currentView: 'dashboard',
  currentTheme: localStorage.getItem('nas_theme') || 'dark',
  isScrubRunning: false,
  isTrimRunning: false,
  activeBackupTimers: [],

  // ============================================================================
  // 1. RECURSOS COMPARTIDOS (SAMBA SHARES)
  // ============================================================================
  shares: [
    {
      id: 'SISTEMAS',
      name: 'SISTEMAS',
      vis: 'Visible',
      browseable: 'yes',
      scheme: '1',
      schemeName: 'Lectura y Escritura por Grupo',
      groups: ['grp_sistemas'],
      writeList: '',
      readOnly: 'no',
      path: '/srv/nas/SISTEMAS',
      status: 'Activo',
      comment: 'Directorio departamental de Tecnología y Dirección'
    },
    {
      id: 'CAMPANA_UNO_OPERACIONES',
      name: 'CAMPANA_UNO_OPERACIONES',
      vis: 'Visible',
      browseable: 'yes',
      scheme: '2',
      schemeName: 'Solo Lectura General + Escritura Exclusiva',
      groups: ['grp_empleados'],
      writeList: 'grp_sistemas',
      readOnly: 'no',
      path: '/srv/nas/CAMPANA_UNO_OPERACIONES',
      status: 'Activo',
      comment: 'Documentación operativa y plantillas de campaña 1'
    },
    {
      id: 'CAMPANA_DOS_FINANZAS',
      name: 'CAMPANA_DOS_FINANZAS',
      vis: 'Visible',
      browseable: 'yes',
      scheme: '1',
      schemeName: 'Lectura y Escritura por Grupo',
      groups: ['grp_sistemas', 'grp_finanzas'],
      writeList: '',
      readOnly: 'no',
      path: '/srv/nas/CAMPANA_DOS_FINANZAS',
      status: 'Activo',
      comment: 'Libros contables e informes financieros en Excel (+100 puestos)'
    },
    {
      id: 'BACKUPS_WINDOWS$',
      name: 'BACKUPS_WINDOWS$',
      vis: 'Oculto ($)',
      browseable: 'no',
      scheme: '3',
      schemeName: 'Solo Lectura Estricta',
      groups: ['grp_sistemas'],
      writeList: '',
      readOnly: 'yes',
      path: '/srv/nas/BACKUPS_HISTORICOS/windows',
      status: 'Activo',
      comment: 'Repositorio de snapshots CIFS / Active Directory'
    },
    {
      id: 'BACKUPS_LINUX$',
      name: 'BACKUPS_LINUX$',
      vis: 'Oculto ($)',
      browseable: 'no',
      scheme: '3',
      schemeName: 'Solo Lectura Estricta',
      groups: ['grp_sistemas'],
      writeList: '',
      readOnly: 'yes',
      path: '/srv/nas/BACKUPS_HISTORICOS/linux',
      status: 'Activo',
      comment: 'Repositorio de snapshots SSH / Linux de servidores de producción'
    },
    {
      id: 'PUBLICO_DOCUMENTOS',
      name: 'PUBLICO_DOCUMENTOS',
      vis: 'Visible',
      browseable: 'yes',
      scheme: '4',
      schemeName: 'Acceso Público / Invitados (Sin Contraseña)',
      groups: ['Todos (Invitados)'],
      writeList: '',
      readOnly: 'no',
      path: '/srv/nas/PUBLICO',
      status: 'Activo',
      comment: 'Plantillas y formatos públicos de la organización'
    }
  ],

  // ============================================================================
  // 2. USUARIOS DEL SISTEMA Y SAMBA
  // ============================================================================
  users: [
    {
      uid: 'admin_nas',
      fullName: 'Administrador Maestro TI',
      groups: ['sudo', 'adm', 'grp_sistemas'],
      sambaActive: true,
      shell: '/bin/bash',
      lastLogin: 'Hoy 10:14 (Consola Local)'
    },
    {
      uid: 'carlos_m',
      fullName: 'Carlos Morales - Cobranzas C1',
      groups: ['grp_c1_cobranzas', 'grp_empleados'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Hoy 09:30 (SMB Windows)'
    },
    {
      uid: 'laura_s',
      fullName: 'Laura Salinas - Ventas C2',
      groups: ['grp_c2_ventas', 'grp_empleados'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Ayer 18:22 (SMB Windows)'
    },
    {
      uid: 'patricia_r',
      fullName: 'Patricia Ramos - Finanzas',
      groups: ['grp_finanzas', 'grp_empleados'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Hoy 08:45 (SMB Windows)'
    },
    {
      uid: 'mario_v',
      fullName: 'Mario Valenzuela - Auditoría',
      groups: ['grp_auditoria'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Hace 3 días'
    },
    {
      uid: 'backup_svc',
      fullName: 'Servicio de Réplica Automatizado',
      groups: ['grp_sistemas'],
      sambaActive: true,
      shell: '/usr/sbin/nologin',
      lastLogin: 'Hoy 06:00 (Daemon)'
    }
  ],

  // ============================================================================
  // 3. GRUPOS DE SEGURIDAD (grp_*)
  // ============================================================================
  groups: [
    { name: 'grp_sistemas', level: 'Maestro (2770)', members: ['admin_nas', 'backup_svc'], shares: ['SISTEMAS', 'BACKUPS_WINDOWS$', 'BACKUPS_LINUX$', 'CAMPANA_DOS_FINANZAS'] },
    { name: 'grp_empleados', level: 'General Empleados', members: ['carlos_m', 'laura_s', 'patricia_r'], shares: ['CAMPANA_UNO_OPERACIONES'] },
    { name: 'grp_c1_cobranzas', level: 'Departamental C1', members: ['carlos_m'], shares: ['CAMPANA_UNO_OPERACIONES'] },
    { name: 'grp_c2_ventas', level: 'Departamental C2', members: ['laura_s'], shares: ['CAMPANA_DOS_FINANZAS'] },
    { name: 'grp_finanzas', level: 'Departamental Finanzas', members: ['patricia_r'], shares: ['CAMPANA_DOS_FINANZAS'] },
    { name: 'grp_auditoria', level: 'Solo Lectura Auditoría', members: ['mario_v'], shares: ['CAMPANA_UNO_OPERACIONES', 'SISTEMAS'] }
  ],

  // ============================================================================
  // 4. CENTRAL DE RESPALDOS (TAREAS)
  // ============================================================================
  backupTasks: [
    {
      id: 'bkp_win_facturacion',
      proto: 'CIFS / SMB 3.1.1',
      src: '//192.168.1.10/Facturas_EAD',
      user: 'TEAM-JOFRATO\\svc_backup',
      credFile: '/etc/backup-credentials/bkp_win_facturacion.cred',
      cron: '0 2 * * *',
      cronDesc: 'Todos los días a las 02:00 AM',
      retention: 15,
      snapsCount: 14,
      lastRun: 'Hoy 02:00:22',
      lastStatus: 'Éxito',
      sizeLogical: '18.4 GB',
      sizeReal: '2.1 GB',
      dedupRate: '88.6%'
    },
    {
      id: 'bkp_lin_servidor_web',
      proto: 'SSH (rsync -aAXH)',
      src: 'root@192.168.1.20:/var/www/html',
      user: 'root (known_hosts_backup)',
      credFile: '/root/.ssh/known_hosts_backup',
      cron: '30 3 * * *',
      cronDesc: 'Todos los días a las 03:30 AM',
      retention: 10,
      snapsCount: 10,
      lastRun: 'Hoy 03:30:15',
      lastStatus: 'Éxito',
      sizeLogical: '4.2 GB',
      sizeReal: '380 MB',
      dedupRate: '91.0%'
    },
    {
      id: 'bkp_win_contabilidad',
      proto: 'CIFS / SMB 3.1.1',
      src: '//192.168.1.15/Contabilidad$',
      user: 'CONTAB\\admin_finanzas',
      credFile: '/etc/backup-credentials/bkp_win_contabilidad.cred',
      cron: '0 */6 * * *',
      cronDesc: 'Cada 6 horas (00:00, 06:00, 12:00, 18:00)',
      retention: 20,
      snapsCount: 14,
      lastRun: 'Hoy 06:00:10',
      lastStatus: 'Éxito',
      sizeLogical: '32.1 GB',
      sizeReal: '4.8 GB',
      dedupRate: '85.1%'
    }
  ],

  // Snapshots por Tarea
  snapshotsData: {
    'bkp_win_facturacion': [
      { name: 'snapshot_2026-10-02_020000', date: '2026-10-02 02:00:22', apparent: '18.4 GB', real: '142 MB', dedup: '99.2%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-10-01_020000', date: '2026-10-01 02:00:19', apparent: '18.3 GB', real: '210 MB', dedup: '98.8%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-09-30_020000', date: '2026-09-30 02:00:24', apparent: '18.2 GB', real: '185 MB', dedup: '99.0%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-09-29_020000', date: '2026-09-29 02:00:15', apparent: '18.1 GB', real: '310 MB', dedup: '98.3%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-09-28_020000', date: '2026-09-28 02:00:18', apparent: '18.0 GB', real: '1.2 GB', dedup: '93.3%', status: 'Snapshot Base' }
    ],
    'bkp_lin_servidor_web': [
      { name: 'snapshot_2026-10-02_033000', date: '2026-10-02 03:30:15', apparent: '4.2 GB', real: '32 MB', dedup: '99.2%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-10-01_033000', date: '2026-10-01 03:30:12', apparent: '4.1 GB', real: '45 MB', dedup: '98.9%', status: 'Atómico / Íntegro' }
    ],
    'bkp_win_contabilidad': [
      { name: 'snapshot_2026-10-02_060000', date: '2026-10-02 06:00:10', apparent: '32.1 GB', real: '84 MB', dedup: '99.7%', status: 'Atómico / Íntegro' },
      { name: 'snapshot_2026-10-02_000000', date: '2026-10-02 00:00:08', apparent: '32.0 GB', real: '115 MB', dedup: '99.6%', status: 'Atómico / Íntegro' }
    ]
  },

  // ============================================================================
  // 5. REGISTROS DEL SISTEMA (LOGS / JOURNALCTL)
  // ============================================================================
  systemLogs: [
    { time: '11:15:02', level: 'INFO', src: 'smbd', text: 'smbd[1420]: Conexión exitosa desde 10.10.1.45 (Windows 11) recurso [CAMPANA_DOS_FINANZAS] usuario [patricia_r]' },
    { time: '11:14:58', level: 'INFO', src: 'wsdd2', text: 'wsdd2[812]: Sonda LLMNR/WSD respondida para host SRV-NAS hacia cliente 10.10.1.45' },
    { time: '11:00:00', level: 'OK',   src: 'kernel', text: 'sysctl: vm.dirty_bytes=268435456 (256MB) y vfs_cache_pressure=30 activos sin saturación de I/O' },
    { time: '08:30:00', level: 'WARN', src: 'smbd', text: 'smbd[1388]: Intento de autenticación fallido para usuario [invitado] desde 10.10.1.88 (NT_STATUS_WRONG_PASSWORD)' },
    { time: '06:00:14', level: 'OK',   src: 'backup', text: 'backup_runner: Tarea [bkp_win_contabilidad] finalizada exitosamente en 14.8s. Snapshots: 14/20' },
    { time: '03:30:18', level: 'OK',   src: 'backup', text: 'backup_runner: Tarea [bkp_lin_servidor_web] rsync con StrictHostKeyChecking=accept-new exitoso' },
    { time: '02:00:25', level: 'OK',   src: 'backup', text: 'backup_runner: Tarea [bkp_win_facturacion] deduplicación 88.6% (0 bytes adicionales en inodos compartidos)' },
    { time: '01:00:00', level: 'INFO', src: 'cron',   text: 'cron[620]: Verificación preventiva de espacio en disco en /srv/nas: 35% de ocupación (umbral seguro <85%)' }
  ],

  // ============================================================================
  // 6. SERVICIOS SYSTEMD
  // ============================================================================
  services: [
    { name: 'smbd.service', desc: 'Samba SMB/CIFS File Server', status: 'active', sub: 'running', enabled: 'enabled', canRestart: true },
    { name: 'nmbd.service', desc: 'Samba NetBIOS Name Server', status: 'active', sub: 'running', enabled: 'enabled', canRestart: true },
    { name: 'wsdd2.service', desc: 'Web Services Discovery Daemon (WSD/LLMNR)', status: 'active', sub: 'running', enabled: 'enabled', canRestart: true },
    { name: 'cron.service', desc: 'Periodic Command Scheduler (nas-backups)', status: 'active', sub: 'running', enabled: 'enabled', canRestart: true },
    { name: 'cockpit.socket', desc: 'Cockpit Web Console Socket', status: 'active', sub: 'listening', enabled: 'enabled', canRestart: true },
    { name: 'fstrim.timer', desc: 'Discard Unused Flash Blocks on SSDs', status: 'active', sub: 'waiting', enabled: 'enabled', canRestart: true },
    { name: 'ssh.service', desc: 'OpenSSH Remote Secure Tunnel Daemon', status: 'active', sub: 'running', enabled: 'enabled', canRestart: true }
  ],

  // ============================================================================
  // 7. NAVEGADOR DE ARCHIVOS (/srv/nas)
  // ============================================================================
  fileBrowser: {
    currentPath: '/srv/nas',
    fileTree: {
      '/srv': [
        { name: 'nas', type: 'dir', size: '4.0 TB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 10:14' }
      ],
      '/srv/nas': [
        { name: 'SISTEMAS', type: 'dir', size: '14.2 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 10:14' },
        { name: 'CAMPANA_UNO_OPERACIONES', type: 'dir', size: '28.6 GB', owner: 'carlos_m', group: 'grp_empleados', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-01 17:30' },
        { name: 'CAMPANA_DOS_FINANZAS', type: 'dir', size: '42.1 GB', owner: 'patricia_r', group: 'grp_finanzas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 08:45' },
        { name: 'BACKUPS_HISTORICOS', type: 'dir', size: '550.0 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 06:00' },
        { name: 'PUBLICO', type: 'dir', size: '1.2 GB', owner: 'nobody', group: 'nogroup', perms: 'drwxrwxrwx', octal: '0777', mtime: '2026-09-28 14:00' },
        { name: 'README_ALMACENAMIENTO.txt', type: 'file', size: '3.4 KB', owner: 'admin_nas', group: 'grp_sistemas', perms: '-rw-rw-r--', octal: '0664', mtime: '2026-09-25 11:20' }
      ],
      '/srv/nas/SISTEMAS': [
        { name: 'scripts_mantenimiento', type: 'dir', size: '45 MB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-01 12:00' },
        { name: 'politicas_seguridad_2026.pdf', type: 'file', size: '1.8 MB', owner: 'admin_nas', group: 'grp_sistemas', perms: '-rw-rw-r--', octal: '0660', mtime: '2026-09-20 09:15' },
        { name: 'inventario_servidores_ead.xlsx', type: 'file', size: '540 KB', owner: 'admin_nas', group: 'grp_sistemas', perms: '-rw-rw-r--', octal: '0660', mtime: '2026-10-02 10:10' }
      ],
      '/srv/nas/SISTEMAS/scripts_mantenimiento': [
        { name: 'btrfs_scrub_audit.sh', type: 'file', size: '2.1 KB', owner: 'root', group: 'grp_sistemas', perms: '-rwxr-x---', octal: '0750', mtime: '2026-10-01 11:30' },
        { name: 'fstrim_maintenance.sh', type: 'file', size: '1.4 KB', owner: 'root', group: 'grp_sistemas', perms: '-rwxr-x---', octal: '0750', mtime: '2026-09-28 09:10' }
      ],
      '/srv/nas/CAMPANA_UNO_OPERACIONES': [
        { name: 'manuales_procedimientos', type: 'dir', size: '12.4 GB', owner: 'carlos_m', group: 'grp_empleados', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-01 16:20' },
        { name: 'plantilla_cobranzas_c1.xlsx', type: 'file', size: '2.4 MB', owner: 'carlos_m', group: 'grp_c1_cobranzas', perms: '-rw-rw-r--', octal: '0664', mtime: '2026-10-01 15:40' }
      ],
      '/srv/nas/CAMPANA_UNO_OPERACIONES/manuales_procedimientos': [
        { name: 'guia_campana_c1_2026.docx', type: 'file', size: '3.1 MB', owner: 'carlos_m', group: 'grp_empleados', perms: '-rw-rw-r--', octal: '0664', mtime: '2026-09-22 14:15' }
      ],
      '/srv/nas/CAMPANA_DOS_FINANZAS': [
        { name: 'estados_financieros_2026.xlsx', type: 'file', size: '14.8 MB', owner: 'patricia_r', group: 'grp_finanzas', perms: '-rw-rw-r--', octal: '0660', mtime: '2026-10-02 08:30' },
        { name: 'balance_general_q3.pdf', type: 'file', size: '3.2 MB', owner: 'patricia_r', group: 'grp_finanzas', perms: '-rw-rw-r--', octal: '0660', mtime: '2026-09-30 18:00' }
      ],
      '/srv/nas/BACKUPS_HISTORICOS': [
        { name: 'windows', type: 'dir', size: '240 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 02:00' },
        { name: 'linux', type: 'dir', size: '180 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 03:30' },
        { name: 'facturacion', type: 'dir', size: '130 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 06:00' }
      ],
      '/srv/nas/BACKUPS_HISTORICOS/windows': [
        { name: 'snapshot_2026-10-02_020000', type: 'dir', size: '18.4 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 02:00' },
        { name: 'snapshot_2026-10-01_020000', type: 'dir', size: '18.3 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-01 02:00' }
      ],
      '/srv/nas/BACKUPS_HISTORICOS/linux': [
        { name: 'snapshot_2026-10-02_033000', type: 'dir', size: '4.2 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 03:30' },
        { name: 'snapshot_2026-10-01_033000', type: 'dir', size: '4.1 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-01 03:30' }
      ],
      '/srv/nas/BACKUPS_HISTORICOS/facturacion': [
        { name: 'snapshot_2026-10-02_060000', type: 'dir', size: '32.1 GB', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---', octal: '2770', mtime: '2026-10-02 06:00' }
      ],
      '/srv/nas/PUBLICO': [
        { name: 'formatos_vacaciones.docx', type: 'file', size: '120 KB', owner: 'nobody', group: 'nogroup', perms: '-rw-rw-rw-', octal: '0666', mtime: '2026-09-15 10:00' },
        { name: 'directorio_telefonico_ead.pdf', type: 'file', size: '450 KB', owner: 'nobody', group: 'nogroup', perms: '-rw-rw-rw-', octal: '0666', mtime: '2026-09-10 11:30' }
      ]
    }
  },

  // ============================================================================
  // 8. ACTUALIZACIONES DE SOFTWARE
  // ============================================================================
  updates: [
    { pkg: 'samba', installed: '4.20.1-Debian', available: '4.20.1-Debian', source: 'debian-trixie-updates', status: 'Actualizado' },
    { pkg: 'wsdd2', installed: '1.8.8-1', available: '1.8.8-1', source: 'debian-trixie-main', status: 'Actualizado' },
    { pkg: 'cockpit', installed: '319-1', available: '319-1', source: 'debian-trixie-backports', status: 'Actualizado' },
    { pkg: 'btrfs-progs', installed: '6.12-1', available: '6.12-1', source: 'debian-trixie-main', status: 'Actualizado' },
    { pkg: 'rsync', installed: '3.3.0-1', available: '3.3.0-1', source: 'debian-trixie-main', status: 'Actualizado' },
    { pkg: 'nas_debian (core)', installed: 'v1.2.4-stable (f0eb31e)', available: 'v1.2.4-stable', source: 'github.com/team-jofrato', status: 'Versión Oficial' }
  ],

  // ============================================================================
  // 9. APLICACIONES Y MÓDULOS DEL SERVIDOR
  // ============================================================================
  applications: [
    {
      id: 'samba',
      name: 'Samba 4 CIFS/SMB Server',
      version: '4.20.1-Debian',
      status: 'Activo',
      desc: 'Servicio central de archivos compartidos para clientes Windows y Linux con soporte de módulos VFS acl_xattr y streams_xattr.',
      icon: 'icon-folder'
    },
    {
      id: 'wsdd2',
      name: 'WSDD2 Web Services Discovery',
      version: '1.8.8-Debian',
      status: 'Activo',
      desc: 'Demonio LLMNR y WSD para visibilidad instantánea del servidor en el Explorador de Red de Windows 10/11 sin protocolos obsoletos.',
      icon: 'icon-network'
    },
    {
      id: 'cockpit_backups',
      name: 'Cockpit-Backups (EAD-COL)',
      version: '2.1.0-stable',
      status: 'Activo',
      desc: 'Plugin web integrado en Cockpit para réplicas multiplataforma, deduplicación por Hardlinks y staging atómico.',
      icon: 'icon-shield'
    },
    {
      id: 'cockpit_identities',
      name: '45Drives File Sharing & Identities',
      version: '3.2.1-stable',
      status: 'Activo',
      desc: 'Módulo gráfico de gestión de ACLs POSIX, usuarios locales, credenciales smbpasswd y grupos de seguridad corporativos.',
      icon: 'icon-users'
    },
    {
      id: 'udisks2',
      name: 'UDisks2 Storage Module',
      version: '2.10.1-Debian',
      status: 'Activo',
      desc: 'Monitoreo de almacenamiento con reglas udev de ocultamiento del disco de sistema operativo (UDISKS_IGNORE=1).',
      icon: 'icon-hard-drive'
    },
    {
      id: 'openssh',
      name: 'OpenSSH Secure Server',
      version: '9.2p1-Debian',
      status: 'Activo',
      desc: 'Canal cifrado de réplica con verificación de llaves StrictHostKeyChecking=accept-new en /root/.ssh/known_hosts_backup.',
      icon: 'icon-terminal'
    }
  ],

  // ============================================================================
  // 10. ESTADO DEL DOMINIO ACTIVE DIRECTORY
  // ============================================================================
  domain: {
    isJoined: false,
    realm: '',
    workgroup: 'TEAM-JOFRATO',
    dc: ''
  },

  // ============================================================================
  // 11. HISTORIAL Y TERMINAL INTERACTIVA
  // ============================================================================
  terminal: {
    history: [],
    historyIdx: -1,
    currentDir: '/srv/nas'
  },

  // Feed de eventos del Dashboard
  activityFeed: [
    { badge: 'Éxito', badgeClass: 'badge-ok', title: 'Snapshot completado: bkp_win_facturacion', desc: 'Deduplicación 88.6% • 0 bytes adicionales en inodos compartidos' },
    { badge: 'Samba', badgeClass: 'badge-blue', title: 'Sesión SMB iniciada por carlos_m desde 10.10.1.34', desc: 'Acceso concedido a recurso [CAMPANA_UNO_OPERACIONES]' },
    { badge: 'WSDD2', badgeClass: 'badge-blue', title: 'Descubrimiento de red WSD respondido para host SRV-NAS', desc: 'Visible en explorador de Windows 10/11 sin SMBv1 ni NetBIOS obsoleto' },
    { badge: 'Scrub', badgeClass: 'badge-ok', title: 'Auditoría mensual Btrfs: 1,420,892 bloques validados', desc: '0 errores de corrupción silenciosa detectados en /srv/nas' }
  ]
};

// ==============================================================================
// Inicialización y Manejo del DOM
// ==============================================================================
document.addEventListener('DOMContentLoaded', () => {
  initTheme();
  setupNavigation();
  renderDashboardActivityFeed();
  renderLogsTable();
  renderServicesTable();
  renderFileBrowser();
  renderSharesTable();
  renderUsersTable();
  renderGroupsTable();
  renderGroupCheckboxes();
  renderBackupTasksTable();
  renderUpdatesTable();
  renderApplicationsGrid();
  updateAllCounters();
  setupModals();
  setupForms();
  setupTerminal();
  updateNewSharePreview();
});

// ==============================================================================
// Actualización Dinámica de Contadores y Badges
// ==============================================================================
function updateAllCounters() {
  const badgeShares = document.getElementById('badge-shares');
  if (badgeShares) badgeShares.innerText = AppState.shares.length;

  const badgeUsers = document.getElementById('badge-users');
  if (badgeUsers) badgeUsers.innerText = AppState.users.length;

  const badgeBackups = document.getElementById('badge-backups');
  if (badgeBackups) badgeBackups.innerText = AppState.backupTasks.length;

  const badgeServices = document.getElementById('badge-services');
  if (badgeServices) {
    const activeCount = AppState.services.filter(s => s.status === 'active').length;
    badgeServices.innerText = `${activeCount}/${AppState.services.length}`;
  }

  const subtabUsers = document.getElementById('subtab-users-label');
  if (subtabUsers) subtabUsers.innerText = `Usuarios del Sistema y Samba (${AppState.users.length})`;

  const subtabGroups = document.getElementById('subtab-groups-label');
  if (subtabGroups) subtabGroups.innerText = `Grupos de Seguridad grp_* (${AppState.groups.length})`;

  // Chips de filtro de recursos Samba
  const totalShares = AppState.shares.length;
  const visibleShares = AppState.shares.filter(s => s.vis === 'Visible').length;
  const hiddenShares = AppState.shares.filter(s => s.vis !== 'Visible' || s.name.endsWith('$')).length;
  const activeShares = AppState.shares.filter(s => s.status === 'Activo').length;
  const disabledShares = totalShares - activeShares;

  document.querySelectorAll('#shares-chips .chip-btn').forEach(btn => {
    const filter = btn.getAttribute('data-filter');
    if (filter === 'all') btn.innerText = `Todos (${totalShares})`;
    else if (filter === 'visible') btn.innerText = `Visibles (${visibleShares})`;
    else if (filter === 'hidden') btn.innerText = `Ocultos $ (${hiddenShares})`;
    else if (filter === 'active') btn.innerText = `Activos (${activeShares})`;
    else if (filter === 'disabled') btn.innerText = `Deshabilitados (${disabledShares})`;
  });
}

// ==============================================================================
// Tema Oscuro / Claro
// ==============================================================================
function initTheme() {
  document.documentElement.setAttribute('data-theme', AppState.currentTheme);
  updateThemeIcon();
}

function toggleTheme() {
  AppState.currentTheme = AppState.currentTheme === 'dark' ? 'light' : 'dark';
  document.documentElement.setAttribute('data-theme', AppState.currentTheme);
  localStorage.setItem('nas_theme', AppState.currentTheme);
  updateThemeIcon();
  showToast(`Modo ${AppState.currentTheme === 'dark' ? 'Oscuro' : 'Claro'} activado`, 'info');
}

function updateThemeIcon() {
  const iconSpan = document.getElementById('theme-toggle-icon');
  if (iconSpan) {
    iconSpan.innerHTML = AppState.currentTheme === 'dark' 
      ? '<use href="#icon-sun"></use>' 
      : '<use href="#icon-moon"></use>';
  }
  const btn = document.getElementById('btn-theme-toggle');
  if (btn) {
    btn.setAttribute('title', AppState.currentTheme === 'dark' ? 'Cambiar a modo Claro' : 'Cambiar a modo Oscuro');
  }
}

// ==============================================================================
// Navegación entre las 13 Vistas
// ==============================================================================
function setupNavigation() {
  const navItems = document.querySelectorAll('.nav-item');
  const sidebar = document.querySelector('.sidebar');
  const backdrop = document.getElementById('sidebar-backdrop');

  function closeSidebar() {
    if (sidebar) sidebar.classList.remove('open');
    if (backdrop) backdrop.classList.remove('active');
  }

  navItems.forEach(item => {
    item.addEventListener('click', () => {
      const viewId = item.getAttribute('data-view');
      switchView(viewId);
      closeSidebar();
    });
  });

  const mobileToggle = document.getElementById('mobile-menu-btn');
  if (mobileToggle) {
    mobileToggle.addEventListener('click', () => {
      if (sidebar) sidebar.classList.toggle('open');
      if (backdrop) backdrop.classList.toggle('active');
    });
  }

  if (backdrop) {
    backdrop.addEventListener('click', closeSidebar);
  }
}

const VIEW_TITLES = {
  dashboard: 'Vista general',
  logs: 'Registros (Logs)',
  storage: 'Almacenamiento',
  networking: 'Redes',
  services: 'Servicios',
  terminal: 'Terminal',
  filebrowser: 'Navegador de archivos',
  shares: 'Redes compartidas',
  backups: 'Respaldos',
  users: 'Usuarios y grupos',
  updates: 'Actualización software',
  applications: 'Aplicaciones',
  domain: 'Unirse a un dominio'
};

function switchView(viewId) {
  const targetView = document.getElementById(`view-${viewId}`);
  if (!targetView) {
    console.warn(`Vista no encontrada: view-${viewId}`);
    return;
  }

  AppState.currentView = viewId;

  document.querySelectorAll('.nav-item').forEach(item => {
    if (item.getAttribute('data-view') === viewId) {
      item.classList.add('active');
    } else {
      item.classList.remove('active');
    }
  });

  document.querySelectorAll('.view-section').forEach(sec => {
    sec.classList.remove('active');
  });

  targetView.classList.add('active');

  const titleEl = document.getElementById('current-view-title');
  const titleName = VIEW_TITLES[viewId] || viewId;
  if (titleEl) titleEl.innerText = titleName;
  document.title = `Cockpit • ${titleName} • SRV-NAS (Debian 13)`;

  // Acciones al cambiar de vista
  if (viewId === 'terminal') {
    focusTerminalInput();
  } else if (viewId === 'logs') {
    renderLogsTable();
  }
}

// ==============================================================================
// Notificaciones Toast Flotantes (PatternFly 4)
// ==============================================================================
function showToast(message, type = 'info') {
  const container = document.getElementById('toast-container');
  if (!container) return;

  const toast = document.createElement('div');
  toast.className = `toast toast-${type}`;

  const iconName = type === 'success' ? 'check-circle' :
                   type === 'danger'  ? 'x-circle' :
                   type === 'warning' ? 'alert-triangle' : 'info';

  toast.innerHTML = `
    <svg class="icon"><use href="#icon-${iconName}"></use></svg>
    <span>${message}</span>
  `;

  container.appendChild(toast);
  setTimeout(() => {
    if (toast.parentNode) toast.parentNode.removeChild(toast);
  }, 4000);
}

// ==============================================================================
// 1. DASHBOARD: FEED DE ACTIVIDAD & MÉTRICAS
// ==============================================================================
function renderDashboardActivityFeed() {
  const container = document.getElementById('dashboard-activity-feed');
  if (!container) return;

  container.innerHTML = AppState.activityFeed.slice(0, 5).map((item, idx) => {
    const isLast = idx === AppState.activityFeed.length - 1;
    const borderStyle = isLast ? '' : 'border-bottom:1px solid var(--border-light); padding-bottom:8px;';
    return `
      <div style="display:flex; align-items:flex-start; gap:10px; font-size:12px; ${borderStyle}">
        <span class="badge ${item.badgeClass}" style="font-size:10px;">${item.badge}</span>
        <div>
          <div>${item.title}</div>
          <div style="color:var(--text-muted); font-size:11px;">${item.desc}</div>
        </div>
      </div>
    `;
  }).join('');
}

function refreshDashboardMetrics() {
  const btn = document.getElementById('btn-refresh-metrics');
  const icon = document.getElementById('icon-refresh-metrics');
  if (icon) icon.classList.add('spin');
  if (btn) btn.disabled = true;

  setTimeout(() => {
    if (icon) icon.classList.remove('spin');
    if (btn) btn.disabled = false;
    updateAllCounters();
    showToast('Métricas del sistema Debian 13 actualizadas en tiempo real', 'success');
  }, 650);
}

// ==============================================================================
// 2. REGISTROS (LOGS / JOURNALCTL)
// ==============================================================================
let currentLogsFilter = 'all';

function renderLogsTable(filterText = '') {
  const tbody = document.getElementById('logs-table-body');
  if (!tbody) return;

  const filtered = AppState.systemLogs.filter(l => {
    if (currentLogsFilter === 'errors' && l.level !== 'ERR' && l.level !== 'WARN') return false;
    if (currentLogsFilter === 'samba' && l.src !== 'smbd' && l.src !== 'wsdd2') return false;
    if (currentLogsFilter === 'backup' && l.src !== 'backup') return false;
    if (currentLogsFilter === 'kernel' && l.src !== 'kernel' && l.src !== 'cron') return false;

    if (filterText) {
      const q = filterText.toLowerCase();
      return l.text.toLowerCase().includes(q) || l.src.toLowerCase().includes(q) || l.level.toLowerCase().includes(q);
    }
    return true;
  });

  if (filtered.length === 0) {
    tbody.innerHTML = `<tr><td colspan="4" class="empty-msg">No se encontraron registros que coincidan con el filtro actual.</td></tr>`;
    return;
  }

  tbody.innerHTML = filtered.map(l => {
    const badgeClass = l.level === 'OK' ? 'badge-ok' :
                       l.level === 'WARN' ? 'badge-warn' :
                       l.level === 'ERR' ? 'badge-err' : 'badge-blue';
    return `
      <tr>
        <td style="font-family:var(--font-mono); color:var(--text-muted); font-size:11.5px;">${l.time}</td>
        <td><span class="badge ${badgeClass}">${l.level}</span></td>
        <td><code>${l.src}</code></td>
        <td style="font-family:var(--font-mono); font-size:12px;">${l.text}</td>
      </tr>
    `;
  }).join('');
}

function filterLogs(filterType, btnElement) {
  currentLogsFilter = filterType;
  document.querySelectorAll('#logs-filter-chips .chip-btn').forEach(btn => btn.classList.remove('active'));
  if (btnElement) btnElement.classList.add('active');
  renderLogsTable(document.getElementById('logs-search-input')?.value || '');
}

function clearLogsFilter() {
  currentLogsFilter = 'all';
  const searchInput = document.getElementById('logs-search-input');
  if (searchInput) searchInput.value = '';
  document.querySelectorAll('#logs-filter-chips .chip-btn').forEach(btn => {
    btn.classList.toggle('active', btn.getAttribute('data-filter') === 'all');
  });
  renderLogsTable();
  showToast('Filtro de registros restablecido', 'info');
}

function copySystemLogs() {
  const text = AppState.systemLogs.map(l => `[${l.time}] [${l.src}] [${l.level}] ${l.text}`).join('\n');
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).then(() => {
      showToast('Bitácoras copiadas al portapapeles', 'success');
    }).catch(() => fallbackCopy(text));
  } else {
    fallbackCopy(text);
  }
}

function fallbackCopy(text) {
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand('copy');
    showToast('Bitácoras copiadas al portapapeles', 'success');
  } catch (e) {
    showToast('No se pudo copiar automáticamente', 'warning');
  }
  document.body.removeChild(ta);
}

// ==============================================================================
// 3. ALMACENAMIENTO: BTRFS SCRUB & SSD TRIM
// ==============================================================================
function runBtrfsScrubSimulation() {
  if (AppState.isScrubRunning) {
    showToast('Una auditoría Btrfs Scrub ya se encuentra en ejecución', 'warning');
    return;
  }

  const btn = document.getElementById('btn-run-scrub');
  const bar = document.getElementById('scrub-progress-bar');
  const statusTxt = document.getElementById('scrub-status-text');

  if (!btn || !bar) return;

  AppState.isScrubRunning = true;
  btn.disabled = true;
  statusTxt.innerText = 'Ejecutando btrfs scrub start -B /srv/nas...';
  bar.style.width = '0%';

  let progress = 0;
  const interval = setInterval(() => {
    progress += 25;
    bar.style.width = `${progress}%`;
    if (progress >= 100) {
      clearInterval(interval);
      AppState.isScrubRunning = false;
      btn.disabled = false;
      statusTxt.innerText = '✔ Scrub finalizado: 1,420,892 bloques verificados. 0 errores detectados (Bit Rot 0%).';
      showToast('Auditoría Btrfs Scrub finalizada: Integridad 100% verificada', 'success');
    }
  }, 400);
}

function runFstrimSimulation() {
  if (AppState.isTrimRunning) {
    showToast('El descarte flash fstrim ya está en progreso', 'warning');
    return;
  }

  const btn = document.getElementById('btn-run-trim');
  if (!btn) return;

  AppState.isTrimRunning = true;
  btn.disabled = true;
  showToast('Ejecutando fstrim -va en unidades SSD flash...', 'info');

  setTimeout(() => {
    AppState.isTrimRunning = false;
    btn.disabled = false;
    const txt = document.getElementById('trim-status-text');
    if (txt) txt.innerText = '✔ Último descarte manual exitoso: 114.6 GiB recortados en /srv/nas';
    showToast('fstrim completado: Sectores flash descartados correctamente', 'success');
  }, 1000);
}

// ==============================================================================
// 4. REDES: TEST DE CONECTIVIDAD EN TIEMPO REAL
// ==============================================================================
function runNetworkTest() {
  const hostInput = document.getElementById('net-test-host');
  const host = hostInput ? hostInput.value.trim() : '10.10.1.45';
  const type = document.getElementById('net-test-type')?.value || '445';
  const out = document.getElementById('net-test-result');

  if (!host) {
    showToast('Ingresa una dirección IP o nombre de host válido', 'danger');
    return;
  }

  if (!out) return;
  out.innerHTML = `<span style="color:var(--accent-primary);">Probando conectividad hacia ${host} vía puerto ${type}...</span>`;

  setTimeout(() => {
    let title = '';
    let detail = '';

    if (type === 'ICMP') {
      title = '✔ Respuesta de Ping ICMP exitosa en 1.1 ms';
      detail = '4 paquetes transmitidos, 4 paquetes recibidos, 0% packet loss. RTT avg = 1.1 ms.';
    } else if (type === '445') {
      title = '✔ Puerto 445/tcp (SMB/CIFS) abierto y escuchando';
      detail = 'Handshake TCP establecido en 2.3 ms. Negociado dialecto SMB 3.1.1 con cifrado AES-128-GCM.';
    } else if (type === '22') {
      title = '✔ Puerto 22/tcp (SSH) abierto y escuchando';
      detail = 'Banner SSH-2.0-OpenSSH_9.2p1 Debian 13 recibido. Negociación criptográfica correcta.';
    } else if (type === '5357') {
      title = '✔ Puerto 5357/tcp (WSD Discovery) activo';
      detail = 'Servicio wsdd2 respondiendo sondas de descubrimiento para clientes Windows 10/11.';
    }

    out.innerHTML = `
      <div style="color:var(--accent-success); font-weight:700;">${title}</div>
      <div style="color:var(--text-secondary); margin-top:2px;">Destino: ${host} | Protocolo: ${type}</div>
      <div style="color:var(--text-muted); font-size:11px;">${detail}</div>
    `;
    showToast(`Conectividad con ${host} (${type}) verificada`, 'success');
  }, 650);
}

// ==============================================================================
// 5. SERVICIOS SYSTEMD
// ==============================================================================
function renderServicesTable() {
  const tbody = document.getElementById('services-table-body');
  if (!tbody) return;

  tbody.innerHTML = AppState.services.map(s => {
    const isActive = s.status === 'active';
    const statusBadge = isActive
      ? `<span class="badge badge-ok"><span class="status-dot status-ok"></span> ${s.status} (${s.sub})</span>`
      : `<span class="badge badge-err"><span class="status-dot status-err"></span> ${s.status} (${s.sub})</span>`;

    return `
      <tr>
        <td>
          <strong>${s.name}</strong>
        </td>
        <td>${s.desc}</td>
        <td>${statusBadge}</td>
        <td><span class="badge badge-gray">${s.enabled}</span></td>
        <td>
          <div class="table-actions">
            <button class="btn btn-secondary btn-sm" onclick="restartService('${s.name}')" title="Reiniciar servicio">
              <svg class="icon"><use href="#icon-refresh"></use></svg> Reiniciar
            </button>
            <button class="btn ${isActive ? 'btn-danger' : 'btn-primary'} btn-sm" onclick="toggleService('${s.name}')">
              ${isActive ? 'Detener' : 'Iniciar'}
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function restartService(serviceName) {
  showToast(`Reiniciando ${serviceName}...`, 'info');
  setTimeout(() => {
    showToast(`Servicio [${serviceName}] reiniciado con éxito`, 'success');
    renderServicesTable();
    updateAllCounters();
  }, 600);
}

function toggleService(serviceName) {
  const svc = AppState.services.find(s => s.name === serviceName);
  if (!svc) return;

  if (svc.status === 'active') {
    svc.status = 'inactive';
    svc.sub = 'dead';
    showToast(`Servicio [${serviceName}] detenido`, 'warning');
  } else {
    svc.status = 'active';
    svc.sub = 'running';
    showToast(`Servicio [${serviceName}] iniciado`, 'success');
  }
  renderServicesTable();
  updateAllCounters();
}

// ==============================================================================
// 6. TERMINAL WEB INTERACTIVA
// ==============================================================================
function updateTerminalPrompt() {
  const promptEl = document.querySelector('.terminal-prompt');
  if (promptEl) {
    const cur = AppState.terminal.currentDir;
    const displayDir = cur === '/root' ? '~' : cur;
    promptEl.innerText = `root@SRV-NAS:${displayDir}#`;
  }
}

function setupTerminal() {
  const input = document.getElementById('terminal-cmd-input');
  const output = document.getElementById('cockpit-terminal-output');

  updateTerminalPrompt();

  if (output && output.children.length === 0) {
    appendTerminalOutput([
      'Debian GNU/Linux 13 (trixie) [Linux 6.12.9-amd64 x86_64]',
      'Consola de Administración Web Cockpit • SRV-NAS (10.10.1.2)',
      'Organización: TEAM-JOFRATO • Rol: ARCHIVOS & BACKUP',
      'Escribe "help" o "ayuda" para ver la lista de comandos disponibles.',
      ''
    ].join('\n'), 'term-info');
  }

  if (input) {
    input.addEventListener('keydown', (e) => {
      if (e.ctrlKey && (e.key === 'l' || e.key === 'L')) {
        e.preventDefault();
        clearTerminalScreen();
        return;
      }
      if (e.ctrlKey && (e.key === 'c' || e.key === 'C')) {
        e.preventDefault();
        const cur = AppState.terminal.currentDir === '/root' ? '~' : AppState.terminal.currentDir;
        appendTerminalOutput(`root@SRV-NAS:${cur}# ${input.value}^C`, 'terminal-cmd-entry');
        input.value = '';
        return;
      }
      if (e.key === 'Enter') {
        const cmd = input.value.trim();
        if (cmd) {
          executeTerminalCommand(cmd);
          AppState.terminal.history.push(cmd);
          AppState.terminal.historyIdx = AppState.terminal.history.length;
          input.value = '';
        }
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        if (AppState.terminal.history.length > 0 && AppState.terminal.historyIdx > 0) {
          AppState.terminal.historyIdx--;
          input.value = AppState.terminal.history[AppState.terminal.historyIdx] || '';
        }
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        if (AppState.terminal.historyIdx < AppState.terminal.history.length - 1) {
          AppState.terminal.historyIdx++;
          input.value = AppState.terminal.history[AppState.terminal.historyIdx] || '';
        } else {
          AppState.terminal.historyIdx = AppState.terminal.history.length;
          input.value = '';
        }
      }
    });
  }
}

function focusTerminalInput() {
  const input = document.getElementById('terminal-cmd-input');
  if (input) input.focus();
}

function clearTerminalScreen() {
  const output = document.getElementById('cockpit-terminal-output');
  if (output) output.innerHTML = '';
  focusTerminalInput();
}

function showTerminalHelp() {
  executeTerminalCommand('help');
}

function appendTerminalOutput(text, className = '') {
  const output = document.getElementById('cockpit-terminal-output');
  if (!output) return;

  const div = document.createElement('div');
  div.className = `terminal-output-line ${className}`;
  div.innerText = text;
  output.appendChild(div);
  output.scrollTop = output.scrollHeight;
}

function executeTerminalCommand(cmd) {
  const cur = AppState.terminal.currentDir === '/root' ? '~' : AppState.terminal.currentDir;
  appendTerminalOutput(`root@SRV-NAS:${cur}# ${cmd}`, 'terminal-cmd-entry');

  const trimmed = cmd.trim();
  const lower = trimmed.toLowerCase();

  if (!trimmed) return;

  if (lower === 'clear' || lower === 'cls') {
    clearTerminalScreen();
    return;
  }

  if (lower === 'help' || lower === 'ayuda') {
    appendTerminalOutput([
      'Comandos del Sistema y Plataforma NAS Debian 13:',
      '  nas --status          : Estado completo de servicios y almacenamiento',
      '  df -h                 : Reporte de sistemas de archivos montados',
      '  free -m               : Uso de memoria RAM y buffers VFS',
      '  ip a                  : Configuración y direcciones IP de interfaces',
      '  systemctl status <svc>: Diagnóstico de demonios (smbd, nmbd, wsdd2, cron, ssh, cockpit)',
      '  btrfs scrub status    : Auditoría contra Bit Rot en /srv/nas',
      '  testparm -s           : Verificación de sintaxis de smb.conf',
      '  ls / ls -la           : Listar archivos en directorio actual',
      '  cd <directorio>       : Cambiar directorio de trabajo',
      '  pwd                   : Imprimir ruta de trabajo actual',
      '  whoami / id           : Identidad y credenciales POSIX del usuario',
      '  ping <host>           : Probar conectividad con paquetes ICMP',
      '  cat <archivo>         : Inspeccionar contenido de archivos del sistema',
      '  uptime / uname -a     : Información de carga, tiempo y kernel',
      '  clear                 : Limpiar pantalla de la consola (Ctrl+L)'
    ].join('\n'), 'term-info');
    return;
  }

  if (lower === 'pwd') {
    appendTerminalOutput(AppState.terminal.currentDir, 'term-cmd');
    return;
  }

  if (lower === 'whoami') {
    appendTerminalOutput('root', 'term-cmd');
    return;
  }

  if (lower === 'hostname') {
    appendTerminalOutput('SRV-NAS', 'term-cmd');
    return;
  }

  if (lower === 'id') {
    appendTerminalOutput('uid=0(root) gid=0(root) groups=0(root),1000(admin_nas),2000(grp_sistemas)', 'term-cmd');
    return;
  }

  if (lower === 'date') {
    appendTerminalOutput(new Date().toUTCString(), 'term-cmd');
    return;
  }

  if (lower.startsWith('ping')) {
    const parts = trimmed.split(/\s+/);
    const target = (parts.length > 1 && !parts[parts.length - 1].startsWith('-')) ? parts[parts.length - 1] : '10.10.1.1';
    appendTerminalOutput([
      `PING ${target} (${target}) 56(84) bytes of data.`,
      `64 bytes from ${target}: icmp_seq=1 ttl=64 time=0.342 ms`,
      `64 bytes from ${target}: icmp_seq=2 ttl=64 time=0.298 ms`,
      `64 bytes from ${target}: icmp_seq=3 ttl=64 time=0.315 ms`,
      `--- ${target} ping statistics ---`,
      `3 packets transmitted, 3 received, 0% packet loss, time 2045ms`,
      `rtt min/avg/max/mdev = 0.298/0.318/0.342/0.018 ms`
    ].join('\n'), 'term-cmd');
    return;
  }

  if (lower === 'cd' || lower === 'cd ~') {
    AppState.terminal.currentDir = '/root';
    updateTerminalPrompt();
    return;
  }

  if (lower.startsWith('cd ')) {
    const dest = trimmed.slice(3).trim();
    if (dest === '..' || dest === '../') {
      const parts = AppState.terminal.currentDir.split('/').filter(Boolean);
      parts.pop();
      AppState.terminal.currentDir = parts.length ? '/' + parts.join('/') : '/';
    } else if (dest.startsWith('/')) {
      AppState.terminal.currentDir = dest.replace(/\/+$/, '') || '/';
    } else {
      const base = AppState.terminal.currentDir === '/' ? '' : AppState.terminal.currentDir;
      AppState.terminal.currentDir = `${base}/${dest}`.replace(/\/+$/, '');
    }
    updateTerminalPrompt();
    return;
  }

  if (lower === 'ls' || lower.startsWith('ls ') || lower === 'dir') {
    const curPath = AppState.terminal.currentDir;
    const items = AppState.fileBrowser.fileTree[curPath] || [
      { name: 'SISTEMAS', type: 'dir', size: '4.0K', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---' },
      { name: 'CAMPANA_UNO_OPERACIONES', type: 'dir', size: '4.0K', owner: 'carlos_m', group: 'grp_empleados', perms: 'drwxrwx---' },
      { name: 'CAMPANA_DOS_FINANZAS', type: 'dir', size: '4.0K', owner: 'patricia_r', group: 'grp_finanzas', perms: 'drwxrwx---' },
      { name: 'BACKUPS_HISTORICOS', type: 'dir', size: '4.0K', owner: 'root', group: 'grp_sistemas', perms: 'drwxrwx---' },
      { name: 'PUBLICO', type: 'dir', size: '4.0K', owner: 'nobody', group: 'nogroup', perms: 'drwxrwxrwx' }
    ];

    if (lower.includes('-l')) {
      const lines = [`total ${items.length * 4}`];
      items.forEach(i => {
        lines.push(`${i.perms} 1 ${i.owner} ${i.group} ${String(i.size).padStart(8, ' ')} Oct  2 10:14 ${i.name}`);
      });
      appendTerminalOutput(lines.join('\n'), 'term-cmd');
    } else {
      const names = items.map(i => i.type === 'dir' ? i.name + '/' : i.name).join('  ');
      appendTerminalOutput(names, 'term-cmd');
    }
    return;
  }

  if (lower.includes('cat /etc/os-release') || lower.includes('cat /etc/issue')) {
    appendTerminalOutput([
      'PRETTY_NAME="Debian GNU/Linux 13 (trixie)"',
      'NAME="Debian GNU/Linux"',
      'VERSION_ID="13"',
      'VERSION="13 (trixie)"',
      'VERSION_CODENAME=trixie',
      'ID=debian',
      'HOME_URL="debian.org"',
      'SUPPORT_URL="debian.org/support"'
    ].join('\n'), 'term-ok');
    return;
  }

  if (lower.includes('smb.conf')) {
    appendTerminalOutput([
      '# /etc/samba/smb.conf (Generado automáticamente - NAS Debian 13)',
      '[global]',
      '   workgroup = TEAM-JOFRATO',
      '   netbios name = SRV-NAS',
      '   server role = standalone server',
      '   server min protocol = SMB2_10',
      '   server max protocol = SMB3_11',
      '   vfs objects = acl_xattr streams_xattr',
      '   store dos attributes = yes',
      '   inherit permissions = yes',
      '   use sendfile = yes',
      '   aio read size = 16384',
      '   max open files = 65535'
    ].join('\n'), 'term-ok');
    return;
  }

  if (lower.startsWith('nas --status') || lower === 'nas') {
    appendTerminalOutput([
      '======================================================================',
      '   ESTADO DE LA PLATAFORMA NAS DEBIAN 13 (TEAM-JOFRATO)',
      '======================================================================',
      '  Host / NetBIOS     : SRV-NAS (IP: 10.10.1.2)',
      '  Workgroup          : TEAM-JOFRATO',
      '  Punto de Montaje   : /srv/nas (3.88 TB, Btrfs zstd:3, 1.4 TB ocupado)',
      '  Servicio Samba     : ACTIVO (smbd: OK, nmbd: OK, wsdd2: OK)',
      '  Conexiones Activas : 28 puestos concurrentes',
      '  Tuning I/O         : vm.dirty_bytes=256MB • readahead=4096KB',
      '  Central Respaldos  : 3 tareas activas • Deduplicación >85%',
      '======================================================================'
    ].join('\n'), 'term-ok');
    return;
  }

  if (lower.startsWith('df')) {
    appendTerminalOutput([
      'Filesystem      Size  Used Avail Use% Mounted on',
      'udev            7.8G     0  7.8G   0% /dev',
      'tmpfs           1.6G  1.8M  1.6G   1% /run',
      '/dev/sda1       118G   18G   94G  16% /',
      'tmpfs           7.9G     0  7.9G   0% /dev/shm',
      '/dev/sda2       3.9T  1.4T  2.5T  36% /srv/nas'
    ].join('\n'), 'term-cmd');
    return;
  }

  if (lower.startsWith('free')) {
    appendTerminalOutput([
      '               total        used        free      shared  buff/cache   available',
      'Mem:           15890        3840        5850          12        6200       11680',
      'Swap:           4096           0        4096'
    ].join('\n'), 'term-cmd');
    return;
  }

  if (lower.startsWith('ip a') || lower === 'ifconfig') {
    appendTerminalOutput([
      '1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN group default',
      '    inet 127.0.0.1/8 scope host lo',
      '2: enp3s0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc pfifo_fast state UP qlen 1000',
      '    inet 10.10.1.2/24 brd 10.10.1.255 scope global enp3s0'
    ].join('\n'), 'term-cmd');
    return;
  }

  if (lower.includes('systemctl')) {
    let svc = 'smbd.service';
    let svcDesc = 'Samba SMB Daemon';
    if (lower.includes('wsdd2')) { svc = 'wsdd2.service'; svcDesc = 'WSDD2 Web Services Discovery Daemon'; }
    else if (lower.includes('nmbd')) { svc = 'nmbd.service'; svcDesc = 'Samba NetBIOS Nameserver'; }
    else if (lower.includes('cockpit')) { svc = 'cockpit.socket'; svcDesc = 'Cockpit Web Service Socket'; }
    else if (lower.includes('cron')) { svc = 'cron.service'; svcDesc = 'Regular background program processing daemon'; }
    else if (lower.includes('ssh')) { svc = 'ssh.service'; svcDesc = 'OpenSSH server daemon'; }
    else if (lower.includes('fstrim')) { svc = 'fstrim.timer'; svcDesc = 'Discard unused flash blocks on SSDs'; }

    const sObj = AppState.services.find(s => s.name === svc);
    const isAct = sObj ? sObj.status === 'active' : true;

    appendTerminalOutput([
      `● ${svc} - ${svcDesc}`,
      `     Loaded: loaded (/lib/systemd/system/${svc}; enabled; vendor preset: enabled)`,
      `     Active: ${isAct ? 'active (running)' : 'inactive (dead)'} since Sun 2026-09-18 04:00:12 UTC; 14 days ago`,
      `   Main PID: 1420 (${svc.split('.')[0]})`,
      `      Tasks: 12 (limit: 18940)`,
      `     Memory: 48.2M`,
      `     CGroup: /system.slice/${svc}`
    ].join('\n'), isAct ? 'term-ok' : 'term-warn');
    return;
  }

  if (lower.includes('btrfs') || lower.includes('scrub')) {
    appendTerminalOutput([
      'Scrub status for UUID f8a211bc-9910-4c22-9214-38ad591c01b2',
      '  scrub started at Thu Oct  1 02:00:00 2026 and finished after 00:18:42',
      '  total to scrub: 1.42TiB',
      '  rate: 1.30GiB/s',
      '  verified: 1420892 blocks',
      '  errors: 0 (csum: 0, read: 0, super: 0, uncorrectable: 0)'
    ].join('\n'), 'term-ok');
    return;
  }

  if (lower.startsWith('testparm')) {
    appendTerminalOutput([
      'Load smb config files from /etc/samba/smb.conf',
      'Loaded services file OK.',
      'Weak crypto is allowed',
      'Server role: ROLE_STANDALONE',
      'Press enter to see a dump of your service definitions'
    ].join('\n'), 'term-ok');
    return;
  }

  if (lower === 'uptime') {
    appendTerminalOutput(' 11:24:18 up 14 days,  6:22,  1 user,  load average: 0.12, 0.08, 0.05', 'term-cmd');
    return;
  }

  if (lower.startsWith('uname')) {
    appendTerminalOutput('Linux SRV-NAS 6.12.9-amd64 #1 SMP PREEMPT_DYNAMIC Debian 6.12.9-1 x86_64 GNU/Linux', 'term-cmd');
    return;
  }

  // Comando no reconocido
  appendTerminalOutput(`bash: ${cmd}: orden no encontrada. Escribe "help" para ver los comandos soportados.`, 'term-err');
}

// ==============================================================================
// 7. NAVEGADOR DE ARCHIVOS (/srv/nas)
// ==============================================================================
function renderFileBrowser() {
  const breadcrumb = document.getElementById('file-breadcrumb-trail');
  const tbody = document.getElementById('file-browser-table-body');
  if (!tbody || !breadcrumb) return;

  const currentPath = AppState.fileBrowser.currentPath;

  // Migas de pan
  const parts = currentPath.split('/').filter(Boolean);
  let accumulated = '';
  breadcrumb.innerHTML = parts.map((part, idx) => {
    accumulated += `/${part}`;
    const target = accumulated;
    const isLast = idx === parts.length - 1;
    return isLast
      ? `<span style="font-weight:700; color:var(--text-main);">${part}</span>`
      : `<span class="file-breadcrumb-segment" onclick="navigateToFileFolder('${target}')">${part}</span> <span>/</span>`;
  }).join(' ');

  // Lista de archivos
  const items = AppState.fileBrowser.fileTree[currentPath] || [];

  if (items.length === 0) {
    tbody.innerHTML = `<tr><td colspan="6" class="empty-msg">El directorio está vacío.</td></tr>`;
    return;
  }

  tbody.innerHTML = items.map(item => {
    const isDir = item.type === 'dir';
    const iconName = isDir ? 'icon-folder' : 'icon-file';
    const clickAttr = isDir ? `onclick="navigateToFileFolder('${currentPath}/${item.name}')"` : '';
    const cursorStyle = isDir ? 'cursor:pointer; color:var(--accent-primary); font-weight:600;' : '';

    return `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <svg class="icon" style="${isDir ? 'color:var(--accent-primary);' : 'color:var(--text-muted);'}"><use href="#${iconName}"></use></svg>
            <span ${clickAttr} style="${cursorStyle}">${item.name}</span>
          </div>
        </td>
        <td>${item.size}</td>
        <td><code>${item.owner}:${item.group}</code></td>
        <td><span class="tag-pill">${item.perms} (${item.octal})</span></td>
        <td><small style="color:var(--text-secondary);">${item.mtime}</small></td>
        <td>
          <div class="table-actions">
            <button class="btn btn-secondary btn-sm" onclick="inspectFilePerms('${item.name}', '${currentPath}/${item.name}', '${item.owner}', '${item.group}', '${item.perms}', '${item.octal}')" title="Ver ACLs y getfacl">
              ACLs
            </button>
            <button class="btn btn-danger btn-sm" onclick="deleteFileItem('${item.name}')" title="Eliminar">
              <svg class="icon"><use href="#icon-trash"></use></svg>
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function navigateToFileFolder(path) {
  if (!AppState.fileBrowser.fileTree[path]) {
    AppState.fileBrowser.fileTree[path] = [];
  }
  AppState.fileBrowser.currentPath = path;
  renderFileBrowser();
}

function navigateUpFolder() {
  const current = AppState.fileBrowser.currentPath;
  if (current === '/srv/nas' || current === '/srv') {
    showToast('Ya te encuentras en la raíz del almacenamiento (/srv/nas)', 'info');
    return;
  }
  const parts = current.split('/').filter(Boolean);
  if (parts.length > 2) {
    parts.pop();
    AppState.fileBrowser.currentPath = '/' + parts.join('/');
  } else {
    AppState.fileBrowser.currentPath = '/srv/nas';
  }
  renderFileBrowser();
}

function inspectFilePerms(name, fullPath, owner, group, perms, octal) {
  document.getElementById('perms-item-name').innerText = name;
  document.getElementById('perms-item-path').value = fullPath;
  document.getElementById('perms-item-owner').value = `${owner} (UID 1000)`;
  document.getElementById('perms-item-group').value = `${group} (GID 2000)`;

  const aclOutput = [
    `# file: ${fullPath}`,
    `# owner: ${owner}`,
    `# group: ${group}`,
    `# flags: -s-`,
    `user::rwx`,
    `group::rwx`,
    `group:${group}:rwx`,
    `mask::rwx`,
    `other::---`,
    `default:user::rwx`,
    `default:group::rwx`,
    `default:group:${group}:rwx`,
    `default:mask::rwx`,
    `default:other::---`
  ].join('\n');

  document.getElementById('perms-item-getfacl').innerText = aclOutput;
  openModal('modal-file-perms');
}

function deleteFileItem(name) {
  if (confirm(`¿Estás seguro de que deseas eliminar [${name}] de /srv/nas?`)) {
    const currentPath = AppState.fileBrowser.currentPath;
    const list = AppState.fileBrowser.fileTree[currentPath];
    if (list) {
      AppState.fileBrowser.fileTree[currentPath] = list.filter(i => i.name !== name);
      delete AppState.fileBrowser.fileTree[`${currentPath}/${name}`];
      renderFileBrowser();
      showToast(`Elemento [${name}] eliminado de disco`, 'danger');
    }
  }
}

function simulateFileUpload() {
  showToast('Cargando archivo simulado en /srv/nas...', 'info');
  setTimeout(() => {
    const currentPath = AppState.fileBrowser.currentPath;
    if (!AppState.fileBrowser.fileTree[currentPath]) {
      AppState.fileBrowser.fileTree[currentPath] = [];
    }
    const list = AppState.fileBrowser.fileTree[currentPath];
    list.push({
      name: `documento_cargado_${Date.now().toString().slice(-4)}.pdf`,
      type: 'file',
      size: '1.4 MB',
      owner: 'admin_nas',
      group: 'grp_sistemas',
      perms: '-rw-rw-r--',
      octal: '0664',
      mtime: 'Hace un momento'
    });
    renderFileBrowser();
    showToast('Archivo subido correctamente con permisos POSIX heredados', 'success');
  }, 700);
}

// ==============================================================================
// 8. RECURSOS COMPARTIDOS (SAMBA SHARES)
// ==============================================================================
let currentShareFilter = 'all';

function renderSharesTable(filterText = '') {
  const tbody = document.getElementById('shares-table-body');
  if (!tbody) return;

  const filtered = AppState.shares.filter(s => {
    if (currentShareFilter === 'visible' && s.vis !== 'Visible') return false;
    if (currentShareFilter === 'hidden' && s.vis !== 'Oculto ($)' && !s.name.endsWith('$')) return false;
    if (currentShareFilter === 'active' && s.status !== 'Activo') return false;
    if (currentShareFilter === 'disabled' && s.status === 'Activo') return false;

    if (filterText) {
      const q = filterText.toLowerCase();
      return s.name.toLowerCase().includes(q) ||
             s.path.toLowerCase().includes(q) ||
             s.comment.toLowerCase().includes(q) ||
             s.groups.some(g => g.toLowerCase().includes(q));
    }
    return true;
  });

  if (filtered.length === 0) {
    tbody.innerHTML = `<tr><td colspan="7" class="empty-msg">No se encontraron recursos que coincidan con la búsqueda.</td></tr>`;
    return;
  }

  tbody.innerHTML = filtered.map(s => {
    const isHidden = s.name.endsWith('$') || s.vis === 'Oculto ($)';
    const isChecked = s.status === 'Activo' ? 'checked' : '';
    const badgeVis = isHidden 
      ? `<span class="badge badge-gray"><svg class="icon icon-sm"><use href="#icon-eye-off"></use></svg> Oculto ($)</span>`
      : `<span class="badge badge-ok"><svg class="icon icon-sm"><use href="#icon-eye"></use></svg> Visible</span>`;

    const groupsHtml = s.groups.map(g => `<span class="tag-pill">${g}</span>`).join(' ');
    const writeListHtml = s.writeList ? `<br><small style="color:var(--accent-warning);">Escritura: <b>${s.writeList}</b></small>` : '';

    return `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <svg class="icon" style="color:var(--accent-primary);"><use href="#icon-folder"></use></svg>
            <div>
              <strong>${s.name}</strong>
              <div style="font-size:11px; color:var(--text-muted);">\\\\10.10.1.2\\${s.name}</div>
            </div>
          </div>
        </td>
        <td>${badgeVis}</td>
        <td>
          <div style="font-size:12px; font-weight:600;">${s.schemeName}</div>
          <div style="font-size:11px; color:var(--text-secondary);">${s.comment}</div>
        </td>
        <td>
          ${groupsHtml}
          ${writeListHtml}
        </td>
        <td><code style="color:var(--accent-cyan); font-size:11.5px;">${s.path}</code></td>
        <td>
          <label class="toggle-switch">
            <input type="checkbox" ${isChecked} onchange="toggleShareStatus('${s.id}')">
            <span class="toggle-slider"></span>
          </label>
        </td>
        <td>
          <div class="table-actions">
            <button class="btn btn-secondary btn-sm" onclick="viewShareConfig('${s.id}')" title="Ver Configuración smb.conf">
              <svg class="icon"><use href="#icon-file"></use></svg>
            </button>
            <button class="btn btn-danger btn-sm" onclick="deleteShare('${s.id}')" title="Eliminar Recurso">
              <svg class="icon"><use href="#icon-trash"></use></svg>
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function filterShares(type, btnElement) {
  currentShareFilter = type;
  document.querySelectorAll('#shares-chips .chip-btn').forEach(btn => btn.classList.remove('active'));
  if (btnElement) btnElement.classList.add('active');
  renderSharesTable(document.getElementById('shares-search-input')?.value || '');
}

function toggleShareStatus(shareId) {
  const share = AppState.shares.find(s => s.id === shareId);
  if (!share) return;
  share.status = share.status === 'Activo' ? 'Deshabilitado' : 'Activo';
  showToast(`Recurso [${share.name}] ${share.status.toLowerCase()} en Samba`, share.status === 'Activo' ? 'success' : 'warning');
  updateAllCounters();
  renderSharesTable(document.getElementById('shares-search-input')?.value || '');
}

function deleteShare(shareId) {
  if (confirm(`¿Estás seguro de que deseas eliminar el recurso compartido [${shareId}]?`)) {
    AppState.shares = AppState.shares.filter(s => s.id !== shareId);
    AppState.groups.forEach(g => {
      g.shares = g.shares.filter(s => s !== shareId);
    });
    showToast(`Recurso [${shareId}] eliminado de smb.conf`, 'danger');
    updateAllCounters();
    renderSharesTable(document.getElementById('shares-search-input')?.value || '');
    renderGroupsTable();
  }
}

function viewShareConfig(shareId) {
  const share = AppState.shares.find(s => s.id === shareId);
  if (!share) return;

  const smbSnippet = generateSmbConfSnippet(share);
  document.getElementById('modal-share-preview-code').innerText = smbSnippet;
  openModal('modal-share-preview');
}

function updateNewSharePreview() {
  const nameInput = document.getElementById('new-share-name');
  if (!nameInput) return;

  const isHiddenRadio = document.getElementById('new-share-vis-hidden');
  const isHidden = isHiddenRadio ? isHiddenRadio.checked : false;
  const pathInput = document.getElementById('new-share-path');
  const commentInput = document.getElementById('new-share-comment');
  const schemeVal = document.querySelector('input[name="new-share-scheme"]:checked')?.value || '1';

  let rawName = nameInput.value.trim().replace(/\s+/g, '_').replace(/[^A-Za-z0-9_$-]/g, '');
  if (isHidden) {
    if (!rawName.endsWith('$')) rawName += '$';
  } else {
    rawName = rawName.replace(/\$$/, '');
  }

  const cleanDir = rawName.replace(/\$$/, '');
  if (pathInput && !pathInput.dataset.userEdited && cleanDir) {
    pathInput.value = `/srv/nas/${cleanDir}`;
  }

  const selectedGroups = Array.from(document.querySelectorAll('.new-share-grp-chk:checked')).map(c => c.value);

  const previewObj = {
    name: rawName || 'RECURSO_NUEVO',
    path: (pathInput && pathInput.value) || '/srv/nas/RECURSO',
    comment: (commentInput && commentInput.value) || 'Carpeta compartida',
    browseable: isHidden ? 'no' : 'yes',
    scheme: schemeVal,
    groups: selectedGroups.length ? selectedGroups : ['grp_sistemas'],
    writeList: document.getElementById('new-share-writelist-select')?.value || 'grp_sistemas'
  };

  const code = generateSmbConfSnippet(previewObj);
  const previewBox = document.getElementById('new-share-smb-preview');
  if (previewBox) previewBox.innerText = code;
}

function generateSmbConfSnippet(s) {
  let validUsersStr = '';
  let writeListStr = '';
  let readOnlyStr = 'no';
  let guestOkStr = 'no';
  let maskStr = '0770';

  if (s.scheme === '1') {
    readOnlyStr = 'no';
    validUsersStr = s.groups && s.groups.length ? s.groups.map(g => `@${g}`).join(' ') : '@grp_sistemas';
  } else if (s.scheme === '2') {
    readOnlyStr = 'no';
    validUsersStr = s.groups && s.groups.length ? s.groups.map(g => `@${g}`).join(' ') : '@grp_sistemas';
    writeListStr = `@${s.writeList || 'grp_sistemas'}`;
  } else if (s.scheme === '3') {
    readOnlyStr = 'yes';
    validUsersStr = s.groups && s.groups.length ? s.groups.map(g => `@${g}`).join(' ') : '@grp_sistemas';
  } else if (s.scheme === '4') {
    readOnlyStr = 'no';
    guestOkStr = 'yes';
    maskStr = '0777';
  }

  const lines = [
    `[${s.name}]`,
    `   comment = ${s.comment || 'Carpeta de Red'}`,
    `   path = ${s.path}`,
    `   browseable = ${s.browseable}`,
    `   available = yes`,
    `   read only = ${readOnlyStr}`,
    `   guest ok = ${guestOkStr}`
  ];

  if (validUsersStr) lines.push(`   valid users = ${validUsersStr}`);
  if (writeListStr) lines.push(`   write list = ${writeListStr}`);

  lines.push(
    `   create mask = ${maskStr}`,
    `   directory mask = ${maskStr}`,
    `   force create mode = ${maskStr}`,
    `   force directory mode = ${maskStr}`,
    `   vfs objects = acl_xattr streams_xattr`,
    `   store dos attributes = yes`,
    `   inherit permissions = yes`
  );

  return lines.join('\n');
}

function renderGroupCheckboxes() {
  const shareGroupsList = document.getElementById('new-share-groups-list');
  if (shareGroupsList) {
    shareGroupsList.innerHTML = AppState.groups.map(g => `
      <label style="display:flex; align-items:center; gap:6px; font-size:12px; cursor:pointer;">
        <input type="checkbox" class="new-share-grp-chk" value="${g.name}" ${g.name === 'grp_sistemas' ? 'checked' : ''}>
        ${g.name} ${g.name === 'grp_sistemas' ? '(Admin)' : ''}
      </label>
    `).join('');

    shareGroupsList.querySelectorAll('.new-share-grp-chk').forEach(c => {
      c.addEventListener('change', () => {
        updateWriteListOptions();
        updateNewSharePreview();
      });
    });
  }

  updateWriteListOptions();

  const userGroupsList = document.getElementById('new-user-groups-list');
  if (userGroupsList) {
    userGroupsList.innerHTML = AppState.groups.map(g => `
      <label style="display:flex; align-items:center; gap:6px; font-size:12px; cursor:pointer;">
        <input type="checkbox" class="new-user-grp-chk" value="${g.name}" ${g.name === 'grp_empleados' ? 'checked' : ''}>
        ${g.name}
      </label>
    `).join('');
  }
}

function updateWriteListOptions() {
  const writeSelect = document.getElementById('new-share-writelist-select');
  if (!writeSelect) return;

  const checkedGroups = Array.from(document.querySelectorAll('.new-share-grp-chk:checked')).map(c => c.value);
  const groupsToUse = checkedGroups.length ? checkedGroups : AppState.groups.map(g => g.name);

  const prevVal = writeSelect.value;
  writeSelect.innerHTML = groupsToUse.map(g => `<option value="${g}">${g}</option>`).join('');
  if (groupsToUse.includes(prevVal)) {
    writeSelect.value = prevVal;
  }
}

// ==============================================================================
// 9. CENTRAL DE RESPALDOS MULTIPLATAFORMA
// ==============================================================================
function renderBackupTasksTable() {
  const tbody = document.getElementById('backup-tasks-table-body');
  if (!tbody) return;

  tbody.innerHTML = AppState.backupTasks.map(t => {
    const isWindows = t.proto.includes('CIFS');
    const badgeProto = isWindows
      ? `<span class="badge badge-blue"><svg class="icon icon-sm"><use href="#icon-server"></use></svg> CIFS / Windows</span>`
      : `<span class="badge badge-blue"><svg class="icon icon-sm"><use href="#icon-terminal"></use></svg> SSH / Linux</span>`;

    return `
      <tr>
        <td>
          <strong>${t.id}</strong>
          <div style="font-size:11px; color:var(--text-muted);">${t.user}</div>
        </td>
        <td>${badgeProto}</td>
        <td><code style="color:var(--accent-primary); font-size:11.5px;">${t.src}</code></td>
        <td>
          <code>${t.cron}</code>
          <div style="font-size:11px; color:var(--text-secondary);">${t.cronDesc}</div>
        </td>
        <td>${t.retention} snaps</td>
        <td><strong style="color:var(--accent-success);">${t.snapsCount}</strong> en disco</td>
        <td>
          <div>${t.lastRun}</div>
          <span class="badge badge-ok" style="font-size:10px;">✔ ${t.lastStatus}</span>
        </td>
        <td>
          <div class="table-actions">
            <button class="btn btn-primary btn-sm" onclick="runBackupTask('${t.id}')" title="Ejecutar Respaldo Inmediatamente">
              <svg class="icon"><use href="#icon-play"></use></svg> Ejecutar
            </button>
            <button class="btn btn-secondary btn-sm" onclick="openSnapshotsModal('${t.id}')" title="Ver Snapshots Deduplicados">
              <svg class="icon"><use href="#icon-clock"></use></svg> Historial
            </button>
            <button class="btn btn-danger btn-sm" onclick="deleteBackupTask('${t.id}')" title="Eliminar Tarea">
              <svg class="icon"><use href="#icon-trash"></use></svg>
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function deleteBackupTask(taskId) {
  if (confirm(`¿Estás seguro de eliminar la tarea de respaldo ${taskId} y su cronograma asociado?`)) {
    AppState.backupTasks = AppState.backupTasks.filter(t => t.id !== taskId);
    delete AppState.snapshotsData[taskId];
    showToast(`Tarea de backup [${taskId}] eliminada`, 'danger');
    updateAllCounters();
    renderBackupTasksTable();
  }
}

function runBackupTask(taskId) {
  const task = AppState.backupTasks.find(t => t.id === taskId);
  if (!task) return;

  AppState.activeBackupTimers.forEach(id => clearTimeout(id));
  AppState.activeBackupTimers = [];

  const terminal = document.getElementById('backup-runner-terminal');
  terminal.innerHTML = '';
  document.getElementById('runner-task-title').innerText = `${taskId} (${task.proto})`;
  openModal('modal-backup-runner');

  const isWindows = task.proto.includes('CIFS');
  const nowStr = new Date().toISOString().replace(/T/, '_').replace(/:/g, '').slice(0, 15);
  const stagingDir = `/srv/nas/BACKUPS_HISTORICOS/${taskId}/.inprogress_${nowStr}`;

  const lines = [
    { t: 0,    level: 'term-info', text: `[1/8] Adquiriendo candado de exclusión mutua /var/lock/backup_${taskId}.lock... (flock OK)` },
    { t: 400,  level: 'term-info', text: `[2/8] Evaluando capacidad en /srv/nas mediante df -Pk...` },
    { t: 800,  level: 'term-ok',   text: `✔ Ocupación actual: 35%. 2.6 TB libres (>2 GB umbral crítico de aborto). Procediendo.` },
    { t: 1400, level: 'term-info', text: `[3/8] Creando staging atómico temporal: ${stagingDir}` },
    { t: 2000, level: 'term-info', text: isWindows
        ? `[4/8] Conectando a origen Windows CIFS [${task.src}] con credenciales AD (0600 root:root)...`
        : `[4/8] Negociando túnel SSH seguro con host Linux origen y StrictHostKeyChecking=accept-new...` },
    { t: 2600, level: 'term-ok',   text: isWindows
        ? `✔ Montaje temporal CIFS exitoso (ro,vers=3.1.1,noserverino,cache=none,soft,timeo=30) en /mnt/backup_sources/${taskId}`
        : `✔ Llave de host registrada en /root/.ssh/known_hosts_backup. Túnel SSH autenticado sin intermediarios.` },
    { t: 3400, level: 'term-cmd',  text: isWindows
        ? `[5/8] Ejecutando rsync -aAXH --numeric-ids --link-dest=../snapshot_reciente /mnt/backup_sources/${taskId}/ ${stagingDir}/`
        : `[5/8] Ejecutando rsync -aAXH --numeric-ids -v -z --timeout=60 --link-dest=../snapshot_reciente ${task.src}/ ${stagingDir}/` },
    { t: 4400, level: 'term-info', text: `     Analizando árbol de archivos... 41,890 archivos idénticos enlazados vía Hardlinks (0 bytes extra).` },
    { t: 5200, level: 'term-info', text: `     Transfiriendo archivos modificados (45.2 MB) a tasa sostenida...` },
    { t: 6000, level: 'term-ok',   text: `✔ Sincronización rsync completada. Código de retorno: 0 (Sin errores de I/O)` },
    { t: 6600, level: 'term-info', text: `[6/8] Promoción atómica de copia íntegra: mv ${stagingDir} snapshot_${nowStr}` },
    { t: 7200, level: 'term-info', text: `[7/8] Evaluando política de retención (${task.retention} snapshots máximos)... Total: ${task.snapsCount + 1}. Dentro de límite.` },
    { t: 7800, level: 'term-info', text: isWindows
        ? `[8/8] Desmontando recurso CIFS en /mnt/backup_sources/${taskId} y liberando descriptor flock...`
        : `[8/8] Cerrando socket SSH y liberando descriptor de candado flock...` },
    { t: 8400, level: 'term-ok',   text: `======================================================================` },
    { t: 8500, level: 'term-ok',   text: `✔ SNAPSHOT COMPLETADO EXITOSAMENTE. AHORRO POR HARDLINKS: >90%` },
    { t: 8600, level: 'term-ok',   text: `======================================================================` }
  ];

  lines.forEach(l => {
    const timerId = setTimeout(() => {
      const now = new Date().toLocaleTimeString();
      const div = document.createElement('div');
      div.className = 'terminal-line';
      div.innerHTML = `<span class="term-time">[${now}]</span> <span class="${l.level}">${l.text}</span>`;
      terminal.appendChild(div);
      terminal.scrollTop = terminal.scrollHeight;

      if (l.t >= 8600) {
        showToast(`Respaldo de [${taskId}] completado con éxito`, 'success');
        task.lastRun = 'Hace unos instantes';
        task.snapsCount += 1;

        const todayDate = new Date();
        const dateFormatted = `${todayDate.toISOString().slice(0, 10)} ${todayDate.toTimeString().slice(0, 8)}`;
        if (!AppState.snapshotsData[taskId]) AppState.snapshotsData[taskId] = [];
        AppState.snapshotsData[taskId].unshift({
          name: `snapshot_${nowStr}`,
          date: dateFormatted,
          apparent: task.sizeLogical,
          real: '124 MB',
          dedup: '98.7%',
          status: 'Atómico / Íntegro'
        });

        const logTime = todayDate.toTimeString().slice(0, 8);
        AppState.systemLogs.unshift({
          time: logTime,
          level: 'OK',
          src: 'backup',
          text: `backup_runner: Tarea [${taskId}] snapshot snapshot_${nowStr} promovido atómicamente.`
        });
        AppState.activityFeed.unshift({
          badge: 'Éxito',
          badgeClass: 'badge-ok',
          title: `Snapshot completado: ${taskId}`,
          desc: `Promoción atómica • Snapshots en disco: ${task.snapsCount}`
        });

        renderBackupTasksTable();
        renderLogsTable();
        renderDashboardActivityFeed();
      }
    }, l.t);

    AppState.activeBackupTimers.push(timerId);
  });
}

function openSnapshotsModal(taskId) {
  const snaps = AppState.snapshotsData[taskId] || [];
  const tbody = document.getElementById('snapshots-table-body');
  document.getElementById('modal-snapshots-task-name').innerText = taskId;

  if (snaps.length === 0) {
    tbody.innerHTML = `<tr><td colspan="6" class="empty-msg">No hay snapshots históricos registrados todavía para esta tarea.</td></tr>`;
  } else {
    tbody.innerHTML = snaps.map(s => `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <svg class="icon" style="color:var(--accent-primary);"><use href="#icon-clock"></use></svg>
            <strong>${s.name}</strong>
          </div>
        </td>
        <td>${s.date}</td>
        <td>${s.apparent}</td>
        <td><strong style="color:var(--accent-success);">${s.real}</strong></td>
        <td><span class="badge badge-ok">${s.dedup}</span></td>
        <td><span class="badge badge-blue">${s.status}</span></td>
      </tr>
    `).join('');
  }

  openModal('modal-snapshots');
}

// ==============================================================================
// 10. USUARIOS Y GRUPOS
// ==============================================================================
function renderUsersTable(filterText = '') {
  const tbody = document.getElementById('users-table-body');
  if (!tbody) return;

  const filtered = AppState.users.filter(u => {
    if (!filterText) return true;
    const q = filterText.toLowerCase();
    return u.uid.toLowerCase().includes(q) ||
           u.fullName.toLowerCase().includes(q) ||
           u.groups.some(g => g.toLowerCase().includes(q));
  });

  tbody.innerHTML = filtered.map(u => {
    const isSambaOk = u.sambaActive 
      ? `<span class="badge badge-ok"><svg class="icon icon-sm"><use href="#icon-check-circle"></use></svg> Sincronizado</span>`
      : `<span class="badge badge-err"><svg class="icon icon-sm"><use href="#icon-x-circle"></use></svg> Bloqueado</span>`;

    const groupsHtml = u.groups.map(g => `<span class="tag-pill">${g}</span>`).join(' ');

    return `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <div style="width:28px; height:28px; border-radius:50%; background:var(--accent-primary-light); color:var(--accent-primary); display:flex; align-items:center; justify-content:center; font-weight:700; font-size:12px;">
              ${u.uid.charAt(0).toUpperCase()}
            </div>
            <div>
              <strong>${u.uid}</strong>
              <div style="font-size:11px; color:var(--text-muted);">${u.lastLogin}</div>
            </div>
          </div>
        </td>
        <td>${u.fullName}</td>
        <td>${groupsHtml}</td>
        <td>${isSambaOk}</td>
        <td><code style="font-size:11px; color:var(--text-secondary);">${u.shell}</code></td>
        <td>
          <div class="table-actions">
            <button class="btn btn-secondary btn-sm" onclick="openChangePasswordModal('${u.uid}')" title="Cambiar Contraseña">
              <svg class="icon"><use href="#icon-key"></use></svg>
            </button>
            <button class="btn btn-secondary btn-sm" onclick="toggleUserSamba('${u.uid}')" title="${u.sambaActive ? 'Bloquear en Samba' : 'Desbloquear en Samba'}">
              <svg class="icon"><use href="#icon-${u.sambaActive ? 'lock' : 'unlock'}"></use></svg>
            </button>
            <button class="btn btn-danger btn-sm" onclick="deleteUser('${u.uid}')" title="Eliminar Usuario">
              <svg class="icon"><use href="#icon-trash"></use></svg>
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function toggleUserSamba(uid) {
  const user = AppState.users.find(u => u.uid === uid);
  if (!user) return;
  user.sambaActive = !user.sambaActive;
  showToast(`Usuario [${uid}] ${user.sambaActive ? 'habilitado' : 'bloqueado'} en Samba (smbpasswd)`, user.sambaActive ? 'success' : 'warning');
  renderUsersTable(document.getElementById('users-search-input')?.value || '');
}

function deleteUser(uid) {
  if (uid === 'admin_nas') {
    showToast('No se puede eliminar el usuario administrador principal del sistema', 'danger');
    return;
  }
  if (confirm(`¿Eliminar al usuario ${uid} del sistema Debian y de la base de datos Samba?`)) {
    AppState.users = AppState.users.filter(u => u.uid !== uid);
    AppState.groups.forEach(g => {
      g.members = g.members.filter(m => m !== uid);
    });
    showToast(`Usuario [${uid}] eliminado satisfactoriamente`, 'danger');
    updateAllCounters();
    renderUsersTable();
    renderGroupsTable();
  }
}

function renderGroupsTable() {
  const tbody = document.getElementById('groups-table-body');
  if (!tbody) return;

  tbody.innerHTML = AppState.groups.map(g => {
    const membersHtml = g.members.length 
      ? g.members.map(m => `<span class="tag-pill">${m}</span>`).join(' ')
      : '<span style="color:var(--text-muted); font-size:11.5px;">(Sin miembros)</span>';

    const sharesHtml = g.shares.length
      ? g.shares.map(s => `<span class="badge badge-gray">${s}</span>`).join(' ')
      : '<span style="color:var(--text-muted); font-size:11.5px;">(Ninguno)</span>';

    return `
      <tr>
        <td>
          <div style="display:flex; align-items:center; gap:8px;">
            <svg class="icon" style="color:var(--accent-primary);"><use href="#icon-users"></use></svg>
            <strong>${g.name}</strong>
          </div>
        </td>
        <td><span class="badge badge-blue">${g.level}</span></td>
        <td>${membersHtml}</td>
        <td>${sharesHtml}</td>
        <td>
          <div class="table-actions">
            <button class="btn btn-secondary btn-sm" onclick="openEditGroupModal('${g.name}')">
              Gestionar
            </button>
          </div>
        </td>
      </tr>
    `;
  }).join('');
}

function openEditGroupModal(groupName) {
  const group = AppState.groups.find(g => g.name === groupName);
  if (!group) return;

  const nameInput = document.getElementById('edit-group-name');
  const nameDisplay = document.getElementById('edit-group-name-display');
  const membersList = document.getElementById('edit-group-members-list');
  const btnDelete = document.getElementById('btn-delete-group');

  if (nameInput) nameInput.value = groupName;
  if (nameDisplay) nameDisplay.innerText = groupName;

  if (btnDelete) {
    btnDelete.style.display = groupName === 'grp_sistemas' ? 'none' : 'inline-flex';
  }

  if (membersList) {
    membersList.innerHTML = AppState.users.map(u => {
      const isMember = group.members.includes(u.uid);
      return `
        <div class="member-check-item">
          <label class="member-check-label">
            <input type="checkbox" class="edit-group-user-chk" value="${u.uid}" ${isMember ? 'checked' : ''}>
            <span><strong>${u.uid}</strong> <small style="color:var(--text-muted); font-weight:normal;">(${u.fullName})</small></span>
          </label>
        </div>
      `;
    }).join('');
  }

  openModal('modal-edit-group');
}

function deleteCurrentGroup() {
  const groupName = document.getElementById('edit-group-name')?.value;
  if (!groupName || groupName === 'grp_sistemas') {
    showToast('No se puede eliminar el grupo maestro grp_sistemas', 'danger');
    return;
  }

  if (confirm(`¿Estás seguro de eliminar el grupo de seguridad [${groupName}]?`)) {
    AppState.groups = AppState.groups.filter(g => g.name !== groupName);
    AppState.users.forEach(u => {
      u.groups = u.groups.filter(g => g !== groupName);
    });
    AppState.shares.forEach(s => {
      s.groups = s.groups.filter(g => g !== groupName);
    });

    closeModal('modal-edit-group');
    showToast(`Grupo [${groupName}] eliminado`, 'danger');
    updateAllCounters();
    renderGroupsTable();
    renderUsersTable();
    renderGroupCheckboxes();
  }
}

function switchUserTab(tabName) {
  document.getElementById('subtab-users-btn').classList.toggle('active', tabName === 'users');
  document.getElementById('subtab-groups-btn').classList.toggle('active', tabName === 'groups');
  document.getElementById('panel-subtab-users').style.display = tabName === 'users' ? 'block' : 'none';
  document.getElementById('panel-subtab-groups').style.display = tabName === 'groups' ? 'block' : 'none';
}

function generateSecurePassword() {
  const chars = 'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789!@#$%&*';
  let pass = '';
  for (let i = 0; i < 14; i++) {
    pass += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  const passInput = document.getElementById('new-user-pass');
  if (passInput) {
    passInput.value = pass;
    passInput.type = 'text';
  }
  showToast('Contraseña robusta generada automáticamente', 'info');
}

// ==============================================================================
// 11. ACTUALIZACIÓN DE SOFTWARE
// ==============================================================================
function renderUpdatesTable() {
  const tbody = document.getElementById('updates-table-body');
  if (!tbody) return;

  tbody.innerHTML = AppState.updates.map(u => `
    <tr>
      <td><strong>${u.pkg}</strong></td>
      <td><code>${u.installed}</code></td>
      <td><code>${u.available}</code></td>
      <td><span class="badge badge-gray">${u.source}</span></td>
      <td><span class="badge badge-ok">${u.status}</span></td>
    </tr>
  `).join('');
}

function checkForUpdates() {
  showToast('Consultando repositorios de Debian 13 y GitHub...', 'info');
  setTimeout(() => {
    showToast('Todos los paquetes del sistema y la plataforma están en su última versión estable.', 'success');
  }, 800);
}

function runSoftwareUpdate() {
  showToast('Iniciando proceso seguro de actualización con rollback...', 'info');
  setTimeout(() => {
    showToast('Plataforma nas_debian verificada: Código 100% íntegro (v1.2.4-stable).', 'success');
  }, 1200);
}

// ==============================================================================
// 12. APLICACIONES Y MÓDULOS DEL SERVIDOR
// ==============================================================================
function renderApplicationsGrid() {
  const grid = document.getElementById('apps-cards-grid');
  if (!grid) return;

  grid.innerHTML = AppState.applications.map(app => `
    <div class="app-card">
      <div class="app-card-top">
        <div class="app-card-icon">
          <svg class="icon icon-lg"><use href="#${app.icon}"></use></svg>
        </div>
        <div class="app-card-meta">
          <h4>${app.name}</h4>
          <span class="badge badge-ok" style="margin-bottom:4px;">${app.status}</span>
          <p>${app.desc}</p>
        </div>
      </div>
      <div style="display:flex; justify-content:space-between; align-items:center; border-top:1px solid var(--border-color); padding-top:10px;">
        <span style="font-size:11px; font-family:var(--font-mono); color:var(--text-muted);">v${app.version}</span>
        <button class="btn btn-secondary btn-sm" onclick="showToast('Módulo [${app.name}] en ejecución óptima', 'info')">
          Detalles
        </button>
      </div>
    </div>
  `).join('');
}

// ==============================================================================
// 13. UNIRSE A UN DOMINIO ACTIVE DIRECTORY
// ==============================================================================
function executeServerReboot() {
  closeModal('modal-reboot-server');
  showToast('Enviando señal SIGTERM y ejecutando sync en /srv/nas...', 'warning');
  setTimeout(() => {
    showToast('Reinicio del servidor programado correctamente.', 'danger');
  }, 1000);
}

function runNetworkDiagnosticsInTerminal() {
  switchView('terminal');
  executeTerminalCommand('ip a');
  setTimeout(() => executeTerminalCommand('ping -c 3 10.10.1.1'), 300);
}

// ==============================================================================
// Manejo de Modales
// ==============================================================================
function setupModals() {
  document.querySelectorAll('.modal-overlay').forEach(modal => {
    modal.addEventListener('click', (e) => {
      if (e.target === modal) {
        closeModal(modal.id);
      }
    });
  });

  document.querySelectorAll('.modal-close').forEach(btn => {
    btn.addEventListener('click', () => {
      const modal = btn.closest('.modal-overlay');
      if (modal) closeModal(modal.id);
    });
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      const openModalEl = document.querySelector('.modal-overlay.open');
      if (openModalEl) closeModal(openModalEl.id);
    }
  });
}

function openModal(modalId) {
  const modal = document.getElementById(modalId);
  if (modal) modal.classList.add('open');
  if (modalId === 'modal-new-share') {
    updateNewSharePreview();
  }
}

function closeModal(modalId) {
  const modal = document.getElementById(modalId);
  if (modal) modal.classList.remove('open');
}

function openChangePasswordModal(uid) {
  const inputUid = document.getElementById('change-pass-uid');
  if (inputUid) inputUid.value = uid;
  const display = document.getElementById('change-pass-user-display');
  if (display) display.innerText = uid;
  const pass1 = document.getElementById('change-pass-new');
  const pass2 = document.getElementById('change-pass-confirm');
  if (pass1) pass1.value = '';
  if (pass2) pass2.value = '';
  openModal('modal-change-password');
}

// ==============================================================================
// Configuración de Formularios y Eventos
// ==============================================================================
function setupForms() {
  // Búsqueda en recursos compartidos
  const shareSearch = document.getElementById('shares-search-input');
  if (shareSearch) {
    shareSearch.addEventListener('input', (e) => renderSharesTable(e.target.value));
  }

  // Búsqueda en logs
  const logsSearch = document.getElementById('logs-search-input');
  if (logsSearch) {
    logsSearch.addEventListener('input', (e) => renderLogsTable(e.target.value));
  }

  // Búsqueda en usuarios
  const userSearch = document.getElementById('users-search-input');
  if (userSearch) {
    userSearch.addEventListener('input', (e) => renderUsersTable(e.target.value));
  }

  // Eventos de creación de recurso
  const nameInput = document.getElementById('new-share-name');
  if (nameInput) nameInput.addEventListener('input', updateNewSharePreview);

  const pathInput = document.getElementById('new-share-path');
  if (pathInput) {
    pathInput.addEventListener('input', () => {
      pathInput.dataset.userEdited = 'true';
      updateNewSharePreview();
    });
  }

  const commentInput = document.getElementById('new-share-comment');
  if (commentInput) commentInput.addEventListener('input', updateNewSharePreview);

  document.querySelectorAll('input[name="new-share-vis"]').forEach(r => {
    r.addEventListener('change', updateNewSharePreview);
  });

  document.querySelectorAll('input[name="new-share-scheme"]').forEach(r => {
    r.addEventListener('change', (e) => {
      document.querySelectorAll('.radio-tile').forEach(t => t.classList.remove('selected'));
      e.target.closest('.radio-tile').classList.add('selected');
      const scheme2Wrap = document.getElementById('new-share-scheme2-wrapper');
      const groupsWrap = document.getElementById('new-share-groups-container');
      if (scheme2Wrap) scheme2Wrap.style.display = e.target.value === '2' ? 'block' : 'none';
      if (groupsWrap) groupsWrap.style.display = e.target.value === '4' ? 'none' : 'block';
      updateNewSharePreview();
    });
  });

  const writelistSelect = document.getElementById('new-share-writelist-select');
  if (writelistSelect) writelistSelect.addEventListener('change', updateNewSharePreview);

  // Guardar Nuevo Recurso
  const formNewShare = document.getElementById('form-new-share');
  if (formNewShare) {
    formNewShare.addEventListener('submit', (e) => {
      e.preventDefault();
      const rawName = document.getElementById('new-share-name').value.trim();
      const isHidden = document.getElementById('new-share-vis-hidden').checked;
      let finalName = rawName.replace(/\s+/g, '_').replace(/[^A-Za-z0-9_$-]/g, '');
      if (isHidden && !finalName.endsWith('$')) finalName += '$';
      if (!isHidden) finalName = finalName.replace(/\$$/, '');

      if (!finalName) {
        showToast('El nombre del recurso no es válido', 'danger');
        return;
      }

      if (AppState.shares.some(s => s.id === finalName)) {
        showToast(`Ya existe un recurso con el nombre [${finalName}]`, 'danger');
        return;
      }

      const schemeVal = document.querySelector('input[name="new-share-scheme"]:checked')?.value || '1';
      const schemeNames = {
        '1': 'Lectura y Escritura por Grupo',
        '2': 'Solo Lectura General + Escritura Exclusiva',
        '3': 'Solo Lectura Estricta',
        '4': 'Acceso Público / Invitados (Sin Contraseña)'
      };

      const selectedGroups = schemeVal === '4'
        ? ['Todos (Invitados)']
        : (Array.from(document.querySelectorAll('.new-share-grp-chk:checked')).map(c => c.value).length
            ? Array.from(document.querySelectorAll('.new-share-grp-chk:checked')).map(c => c.value)
            : ['grp_sistemas']);

      const newShare = {
        id: finalName,
        name: finalName,
        vis: isHidden ? 'Oculto ($)' : 'Visible',
        browseable: isHidden ? 'no' : 'yes',
        scheme: schemeVal,
        schemeName: schemeNames[schemeVal],
        groups: selectedGroups,
        writeList: schemeVal === '2' ? (document.getElementById('new-share-writelist-select')?.value || 'grp_sistemas') : '',
        readOnly: schemeVal === '3' ? 'yes' : 'no',
        path: document.getElementById('new-share-path').value || `/srv/nas/${finalName.replace(/\$$/, '')}`,
        status: 'Activo',
        comment: document.getElementById('new-share-comment').value || `Carpeta compartida ${finalName}`
      };

      AppState.shares.push(newShare);

      if (schemeVal !== '4') {
        selectedGroups.forEach(gName => {
          const grp = AppState.groups.find(g => g.name === gName);
          if (grp && !grp.shares.includes(finalName)) grp.shares.push(finalName);
        });
      }

      showToast(`Recurso [${finalName}] creado con éxito en smb.conf`, 'success');
      closeModal('modal-new-share');
      updateAllCounters();
      renderSharesTable();
      renderGroupsTable();
      formNewShare.reset();
      if (pathInput) pathInput.dataset.userEdited = '';
    });
  }

  // Guardar Nuevo Usuario
  const formNewUser = document.getElementById('form-new-user');
  if (formNewUser) {
    formNewUser.addEventListener('submit', (e) => {
      e.preventDefault();
      const uid = document.getElementById('new-user-uid').value.trim().toLowerCase();
      const fullName = document.getElementById('new-user-fullname').value.trim();
      const selectedGroups = Array.from(document.querySelectorAll('.new-user-grp-chk:checked')).map(c => c.value);

      if (!uid.match(/^[a-z][a-z0-9_-]*$/)) {
        showToast('El usuario debe empezar con letra minúscula y contener solo letras y números', 'danger');
        return;
      }

      if (AppState.users.some(u => u.uid === uid)) {
        showToast(`El usuario ${uid} ya existe`, 'danger');
        return;
      }

      const assignedGroups = selectedGroups.length ? selectedGroups : ['grp_empleados'];

      AppState.users.push({
        uid: uid,
        fullName: fullName || 'Empleado EAD',
        groups: assignedGroups,
        sambaActive: true,
        shell: '/usr/sbin/nologin',
        lastLogin: 'Nunca (Nuevo)'
      });

      assignedGroups.forEach(gName => {
        const grp = AppState.groups.find(g => g.name === gName);
        if (grp && !grp.members.includes(uid)) grp.members.push(uid);
      });

      showToast(`Usuario [${uid}] creado y sincronizado con Samba`, 'success');
      closeModal('modal-new-user');
      updateAllCounters();
      renderUsersTable();
      renderGroupsTable();
      formNewUser.reset();
    });
  }

  // Guardar Nuevo Grupo
  const formNewGroup = document.getElementById('form-new-group');
  if (formNewGroup) {
    formNewGroup.addEventListener('submit', (e) => {
      e.preventDefault();
      const nameSuffix = document.getElementById('new-group-name').value.trim().toLowerCase().replace(/[^a-z0-9_]/g, '');
      const groupFullName = `grp_${nameSuffix}`;

      if (!nameSuffix) {
        showToast('El nombre del grupo no puede estar vacío', 'danger');
        return;
      }

      if (AppState.groups.some(g => g.name === groupFullName)) {
        showToast(`El grupo ${groupFullName} ya existe`, 'danger');
        return;
      }

      AppState.groups.push({
        name: groupFullName,
        level: 'Departamental',
        members: [],
        shares: []
      });

      showToast(`Grupo de seguridad [${groupFullName}] creado`, 'success');
      closeModal('modal-new-group');
      updateAllCounters();
      renderGroupsTable();
      renderGroupCheckboxes();
      formNewGroup.reset();
    });
  }

  // Guardar Nueva Carpeta en File Browser
  const formNewFolder = document.getElementById('form-new-folder');
  if (formNewFolder) {
    formNewFolder.addEventListener('submit', (e) => {
      e.preventDefault();
      const folderName = document.getElementById('new-folder-name').value.trim().replace(/\s+/g, '_');
      if (!folderName) return;

      if (!AppState.fileBrowser.fileTree[AppState.fileBrowser.currentPath]) {
        AppState.fileBrowser.fileTree[AppState.fileBrowser.currentPath] = [];
      }
      const list = AppState.fileBrowser.fileTree[AppState.fileBrowser.currentPath];
      list.push({
        name: folderName,
        type: 'dir',
        size: '4.0 KB',
        owner: 'root',
        group: 'grp_sistemas',
        perms: 'drwxrwx---',
        octal: '2770',
        mtime: 'Hace un momento'
      });
      AppState.fileBrowser.fileTree[`${AppState.fileBrowser.currentPath}/${folderName}`] = [];
      renderFileBrowser();
      showToast(`Carpeta [${folderName}] creada con permisos 2770`, 'success');
      closeModal('modal-new-folder');
      formNewFolder.reset();
    });
  }

  // Guardar Miembros Editados en Grupo
  const formEditGroup = document.getElementById('form-edit-group');
  if (formEditGroup) {
    formEditGroup.addEventListener('submit', (e) => {
      e.preventDefault();
      const groupName = document.getElementById('edit-group-name').value;
      const grp = AppState.groups.find(g => g.name === groupName);
      if (!grp) return;

      const checkedUids = Array.from(document.querySelectorAll('.edit-group-user-chk:checked')).map(c => c.value);
      grp.members = checkedUids;

      AppState.users.forEach(u => {
        const shouldBeMember = checkedUids.includes(u.uid);
        const isMember = u.groups.includes(groupName);
        if (shouldBeMember && !isMember) u.groups.push(groupName);
        else if (!shouldBeMember && isMember) u.groups = u.groups.filter(g => g !== groupName);
      });

      showToast(`Miembros de [${groupName}] actualizados`, 'success');
      closeModal('modal-edit-group');
      renderGroupsTable();
      renderUsersTable();
    });
  }

  // Guardar Cambio de Clave
  const formChangePassword = document.getElementById('form-change-password');
  if (formChangePassword) {
    formChangePassword.addEventListener('submit', (e) => {
      e.preventDefault();
      const uid = document.getElementById('change-pass-uid')?.value;
      const pass1 = document.getElementById('change-pass-new')?.value;
      const pass2 = document.getElementById('change-pass-confirm')?.value;

      if (!pass1 || pass1.length < 6) {
        showToast('La contraseña debe tener al menos 6 caracteres', 'danger');
        return;
      }

      if (pass1 !== pass2) {
        showToast('Las contraseñas no coinciden. Intenta de nuevo.', 'danger');
        return;
      }

      showToast(`Contraseña de [${uid}] actualizada en Debian y Samba`, 'success');
      closeModal('modal-change-password');
      formChangePassword.reset();
    });
  }

  // Guardar Nueva Tarea de Backup
  const formNewBackup = document.getElementById('form-new-backup');
  if (formNewBackup) {
    formNewBackup.addEventListener('submit', (e) => {
      e.preventDefault();
      const id = document.getElementById('new-bkp-id').value.trim().toLowerCase().replace(/[^a-z0-9_]/g, '');
      const proto = document.getElementById('new-bkp-proto').value;
      const src = document.getElementById('new-bkp-src').value.trim();
      const user = document.getElementById('new-bkp-user').value.trim();
      const cron = document.getElementById('new-bkp-cron').value.trim() || '0 2 * * *';
      const retention = parseInt(document.getElementById('new-bkp-retention').value) || 15;

      if (!id || !src) {
        showToast('Completa todos los campos obligatorios', 'danger');
        return;
      }

      if (AppState.backupTasks.some(t => t.id === id)) {
        showToast(`Ya existe una tarea con el identificador [${id}]`, 'danger');
        return;
      }

      AppState.backupTasks.push({
        id: id,
        proto: proto,
        src: src,
        user: user || 'root',
        credFile: `/etc/backup-credentials/${id}.cred`,
        cron: cron,
        cronDesc: 'Programado vía cron.d',
        retention: retention,
        snapsCount: 0,
        lastRun: 'Pendiente de primer ciclo',
        lastStatus: 'Listo',
        sizeLogical: '0 B',
        sizeReal: '0 B',
        dedupRate: 'N/A'
      });

      AppState.snapshotsData[id] = [];

      showToast(`Tarea de respaldo [${id}] configurada en /etc/cron.d/backup_${id}`, 'success');
      closeModal('modal-new-backup');
      updateAllCounters();
      renderBackupTasksTable();
      formNewBackup.reset();
    });
  }

  // Formulario Unirse a Dominio Active Directory
  const formDomain = document.getElementById('form-domain-join');
  if (formDomain) {
    formDomain.addEventListener('submit', (e) => {
      e.preventDefault();
      const domainName = document.getElementById('ad-domain-name')?.value.trim();
      const dcIp = document.getElementById('ad-dc-ip')?.value.trim();
      const adminUser = document.getElementById('ad-admin-user')?.value.trim();
      const btn = document.getElementById('btn-domain-join');

      if (!domainName || !dcIp || !adminUser) {
        showToast('Completa todos los parámetros del dominio', 'danger');
        return;
      }

      if (btn) btn.disabled = true;
      showToast(`Contactando Controlador de Dominio [${dcIp}] y negociando ticket Kerberos...`, 'info');

      setTimeout(() => {
        if (btn) btn.disabled = false;
        AppState.domain.isJoined = true;
        AppState.domain.realm = domainName;
        AppState.domain.dc = dcIp;

        const badgeDomain = document.getElementById('badge-domain');
        if (badgeDomain) {
          badgeDomain.innerText = 'OK';
          badgeDomain.className = 'nav-badge badge-ok';
        }

        const statusBox = document.getElementById('domain-current-status-box');
        if (statusBox) {
          statusBox.className = 'alert-box alert-box-success';
          statusBox.innerHTML = `
            <svg class="icon icon-lg"><use href="#icon-check-circle"></use></svg>
            <div style="flex:1;">
              <strong>Servidor Integrado en Dominio:</strong> El host <code>SRV-NAS</code> se unió exitosamente al reino Active Directory <b>${domainName}</b>.
              Samba Winbind y SSSD se encuentran sincronizando cuentas de usuario corporativas.
              <div style="margin-top:8px;">
                <button type="button" class="btn btn-secondary btn-sm" onclick="leaveDomain()">
                  <svg class="icon"><use href="#icon-x-circle"></use></svg> Desvincular del Dominio
                </button>
              </div>
            </div>
          `;
        }
        showToast(`Servidor unido con éxito al dominio ${domainName}`, 'success');
      }, 1400);
    });
  }
}

function leaveDomain() {
  AppState.domain.isJoined = false;
  AppState.domain.realm = '';
  AppState.domain.dc = '';

  const badgeDomain = document.getElementById('badge-domain');
  if (badgeDomain) {
    badgeDomain.innerText = 'AD';
    badgeDomain.className = 'nav-badge';
  }

  const statusBox = document.getElementById('domain-current-status-box');
  if (statusBox) {
    statusBox.className = 'alert-box alert-box-info';
    statusBox.innerHTML = `
      <svg class="icon icon-lg"><use href="#icon-info"></use></svg>
      <div>
        <strong>Estado actual:</strong> El servidor opera actualmente en modo <b>Grupo de Trabajo Local</b> (<code>WORKGROUP: TEAM-JOFRATO</code>).
        Para permitir que usuarios corporativos de Active Directory inicien sesión con sus credenciales Windows, configure los parámetros del dominio a continuación.
      </div>
    `;
  }
  showToast('Servidor desvinculado del dominio Active Directory', 'info');
}
