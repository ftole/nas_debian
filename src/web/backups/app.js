/*
 * EAD NAS - Cockpit Backups Plugin
 * Alpine.js logic
 */

document.addEventListener('alpine:init', () => {
    Alpine.data('backupApp', () => ({
        API_PATH: '/usr/share/cockpit/backups/backup_api.py',
        currentTab: 'tasks',
        currentProto: 'cifs',
        
        tasks: [],
        warnings: [],
        loadingTasks: false,

        form: {
            id: '', ip: '', share: '', port: 22, remote_path: '', local_path: '/srv/nas/SISTEMAS',
            user: '', password: '', cron_select: '0 23 * * *', cron_custom: '0 23 * * *', retention: 30
        },
        formSaving: false,
        testStatus: null, // null | { type: 'info|ok|err', msg: '' }

        logTabSelected: '',
        logTabContent: 'Selecciona una tarea...',
        
        modal: { open: false, title: '', content: 'Cargando...' },

        init() {
            this.initTheme();
            this.cargarTareas();
        },

        initTheme() {
            const state = localStorage.getItem("houston-theme-state");
            if (state === "light") document.documentElement.setAttribute("data-theme", "light");
            else document.documentElement.setAttribute("data-theme", "dark");
        },

        async runApi(args) {
            try {
                const out = await cockpit.spawn(["python3", this.API_PATH].concat(args), { superuser: "require", err: "message" });
                return JSON.parse(out.trim());
            } catch (e) {
                return { status: "error", message: e.message || e.problem || String(e) };
            }
        },

        async runApiInput(action, payload) {
            return new Promise((resolve) => {
                const proc = cockpit.spawn(["python3", this.API_PATH, action], { superuser: "require", err: "message" });
                proc.input(JSON.stringify(payload), true);
                proc.then(out => {
                    try { resolve(JSON.parse(out.trim())); }
                    catch (err) { resolve({ status: "error", message: "Respuesta inválida" }); }
                }).catch(e => {
                    resolve({ status: "error", message: e.message || e.problem || String(e) });
                });
            });
        },

        async cargarTareas() {
            this.loadingTasks = true;
            this.tasks = [];
            this.warnings = [];
            const res = await this.runApi(["list"]);
            this.loadingTasks = false;
            if (res.status === 'ok') {
                this.tasks = res.tasks || [];
                this.warnings = res.warnings || [];
            } else {
                alert("Error al cargar tareas: " + res.message);
            }
        },

        async probarConexion() {
            this.testStatus = { type: 'info', msg: 'Probando conexión...' };
            let payload = { ip: this.form.ip.trim(), user: this.form.user.trim(), password: this.form.password };
            let action = 'test_cifs';
            
            if (this.currentProto === 'cifs') {
                payload.share = this.form.share.trim();
            } else if (this.currentProto === 'ssh') {
                action = 'test_ssh';
                payload.port = this.form.port;
            } else {
                return; // Local doesn't need test
            }

            const res = await this.runApiInput(action, payload);
            if (res.status === 'ok') {
                this.testStatus = { type: 'ok', msg: res.message };
            } else {
                this.testStatus = { type: 'err', msg: res.message };
            }
        },

        async guardarTarea() {
            if (!this.form.id.trim()) { alert("Ingresa un identificador para la tarea."); return; }
            this.formSaving = true;
            const cronExpr = this.form.cron_select === 'custom' ? this.form.cron_custom.trim() : this.form.cron_select;
            
            let payload = {
                id: this.form.id.trim(), proto: this.currentProto, 
                cron: cronExpr, retention: parseInt(this.form.retention) || 30
            };

            if (this.currentProto === 'cifs') {
                payload.ip = this.form.ip.trim(); payload.share = this.form.share.trim();
                payload.user = this.form.user.trim(); payload.password = this.form.password;
            } else if (this.currentProto === 'ssh') {
                payload.ip = this.form.ip.trim(); payload.port = this.form.port; 
                payload.path = this.form.remote_path.trim();
                payload.user = this.form.user.trim(); payload.password = this.form.password;
            } else {
                payload.path = this.form.local_path.trim();
            }

            const res = await this.runApiInput("create", payload);
            this.formSaving = false;
            
            if (res.status === 'ok') {
                alert("✔ " + res.message);
                this.resetForm();
                this.currentTab = 'tasks';
                this.cargarTareas();
            } else {
                alert("Error: " + res.message);
            }
        },

        resetForm() {
            this.form = { id: '', ip: '', share: '', port: 22, remote_path: '', local_path: '/srv/nas/SISTEMAS', user: '', password: '', cron_select: '0 23 * * *', cron_custom: '0 23 * * *', retention: 30 };
            this.testStatus = null;
            this.currentProto = 'cifs';
        },

        async ejecutarAhora(id) {
            if(!confirm(`¿Ejecutar backup [${id}] ahora en segundo plano?`)) return;
            const res = await this.runApi(["run", id]);
            alert(res.message);
            this.cargarTareas();
        },
        async abortarTarea(id) {
            if(!confirm(`¿Abortar la ejecución en curso de [${id}]?`)) return;
            const res = await this.runApi(["abort", id]);
            alert(res.message);
            this.cargarTareas();
        },
        async eliminarTarea(id) {
            if(!confirm(`¿Eliminar tarea [${id}]? Los respaldos en disco se conservarán.`)) return;
            const res = await this.runApi(["delete", id, "--confirm"]);
            alert(res.message);
            this.cargarTareas();
        },

        async abrirModalLogs(id) {
            this.modal.title = "Registro: " + id;
            this.modal.content = "Cargando...";
            this.modal.open = true;
            const res = await this.runApi(["logs", id]);
            this.modal.content = res.status === 'ok' ? (res.logs || '(Sin registros)') : ("⚠ Error: " + (res.message || res.logs));
        },

        async verLogs() {
            if (!this.logTabSelected) return;
            this.logTabContent = "Cargando...";
            const res = await this.runApi(["logs", this.logTabSelected]);
            this.logTabContent = res.status === 'ok' ? (res.logs || '(Sin registros)') : ("⚠ Error: " + (res.message || res.logs));
        }
    }));
});
