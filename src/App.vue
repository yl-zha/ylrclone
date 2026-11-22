<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

import { api, errorText, type MountStatus, type RemoteSpec, type SettingsView } from './api'
import RemoteForm from './components/RemoteForm.vue'

const settings = ref<SettingsView | null>(null)
const editing = ref<RemoteSpec | null>(null)
const isNew = ref(false)
const busy = ref<string | null>(null)
const saving = ref(false)
const message = ref<{ kind: 'ok' | 'err' | 'info'; text: string } | null>(null)
const logName = ref<string | null>(null)
const logText = ref('')
const pendingDelete = ref<string | null>(null)

let statusTimer: number | undefined
let logTimer: number | undefined
let deleteTimer: number | undefined

const remotes = computed(() => settings.value?.remotes ?? [])
const statuses = computed(() => settings.value?.statuses ?? [])
const platform = computed(() => settings.value?.platform ?? 'windows')
const winfspWarning = computed(
  () => !!settings.value && settings.value.winfspRequired && !settings.value.winfspOk,
)

function notify(kind: 'ok' | 'err' | 'info', text: string) {
  message.value = { kind, text }
}

function statusOf(name: string): MountStatus | undefined {
  return statuses.value.find((s) => s.name === name)
}

function stateClass(name: string): string {
  const state = statusOf(name)?.state
  if (state === 'mounted') return 'ok'
  if (state === 'error') return 'err'
  return 'muted'
}

function stateText(name: string): string {
  const state = statusOf(name)?.state
  if (state === 'mounted') return '已挂载'
  if (state === 'error') return '异常'
  return '未挂载'
}

async function refresh() {
  try {
    settings.value = await api.settings()
  } catch (e) {
    notify('err', `读取配置失败：${errorText(e)}`)
  }
}

async function refreshStatuses() {
  try {
    const list = await api.statuses()
    if (settings.value) settings.value = { ...settings.value, statuses: list }
  } catch {
    /* 后台轮询失败不打扰用户 */
  }
}

function startNew() {
  isNew.value = true
  editing.value = {
    name: '',
    url: '',
    vendor: '',
    user: '',
    mountPoint: platform.value === 'windows' ? 'X:' : '/mnt/remote',
    subPath: '',
    extraFlags: [],
    autoMount: false,
    networkMode: false,
  }
}

function startEdit(remote: RemoteSpec) {
  isNew.value = false
  editing.value = { ...remote, extraFlags: [...remote.extraFlags] }
}

async function save(spec: RemoteSpec, pass: string | null) {
  saving.value = true
  try {
    await api.saveRemote({ spec, pass })
    notify('ok', `已保存 ${spec.name}`)
    editing.value = null
    await refresh()
  } catch (e) {
    notify('err', `保存失败：${errorText(e)}`)
  } finally {
    saving.value = false
  }
}

async function mount(name: string) {
  busy.value = name
  notify('info', `正在挂载 ${name}…`)
  try {
    const st = await api.mount(name)
    if (st.state === 'mounted') notify('ok', `${name} 已挂载到 ${st.mountPoint}`)
    else notify('err', `${name} 挂载失败：${st.error ?? '未知原因'}`)
    await refresh()
  } catch (e) {
    notify('err', `${name} 挂载失败：${errorText(e)}`)
  } finally {
    busy.value = null
  }
}

async function unmount(name: string) {
  busy.value = name
  try {
    await api.unmount(name)
    notify('ok', `${name} 已卸载`)
    await refresh()
  } catch (e) {
    notify('err', `${name} 卸载失败：${errorText(e)}`)
  } finally {
    busy.value = null
  }
}

function askDelete(name: string) {
  if (pendingDelete.value === name) {
    void doDelete(name)
    return
  }
  pendingDelete.value = name
  notify('info', `再点一次「确认删除」才会移除 ${name}`)
  window.clearTimeout(deleteTimer)
  deleteTimer = window.setTimeout(() => (pendingDelete.value = null), 4000)
}

async function doDelete(name: string) {
  pendingDelete.value = null
  busy.value = name
  try {
    await api.deleteRemote(name)
    notify('ok', `已删除 ${name}`)
    if (editing.value?.name === name) editing.value = null
    await refresh()
  } catch (e) {
    notify('err', `删除失败：${errorText(e)}`)
  } finally {
    busy.value = null
  }
}

async function openLog(name: string) {
  logName.value = name
  await loadLog()
}

async function loadLog() {
  if (!logName.value) return
  try {
    logText.value = await api.logTail(logName.value, 200)
  } catch (e) {
    logText.value = errorText(e)
  }
}

function closeLog() {
  logName.value = null
  logText.value = ''
}

async function toggleAutostart() {
  if (!settings.value) return
  try {
    settings.value = await api.setAutostart(!settings.value.autostart)
    notify('ok', settings.value.autostart ? '已开启开机自启' : '已关闭开机自启')
  } catch (e) {
    notify('err', `设置自启失败：${errorText(e)}`)
  }
}

async function toggleAutoRemount() {
  if (!settings.value) return
  try {
    settings.value = await api.setAutoRemount(!settings.value.autoRemount)
    notify('ok', settings.value.autoRemount ? '已开启掉线自动重挂' : '已关闭掉线自动重挂')
  } catch (e) {
    notify('err', `设置失败：${errorText(e)}`)
  }
}

async function openDataDir() {
  try {
    await api.openDataDir()
  } catch (e) {
    notify('err', errorText(e))
  }
}

onMounted(async () => {
  await refresh()
  statusTimer = window.setInterval(refreshStatuses, 4000)
  logTimer = window.setInterval(() => {
    if (logName.value) void loadLog()
  }, 2500)
})

onBeforeUnmount(() => {
  window.clearInterval(statusTimer)
  window.clearInterval(logTimer)
  window.clearTimeout(deleteTimer)
})
</script>

<template>
  <div class="app">
    <header class="app-header">
      <div class="brand">
        <div
          style="
            width: 30px;
            height: 30px;
            border-radius: 8px;
            background: linear-gradient(145deg, #1e5bff, #123a9e);
            color: #fff;
            font-weight: 800;
            display: grid;
            place-items: center;
            font-size: 16px;
          "
        >
          R
        </div>
        <div>
          <div class="brand-title">ylrclone</div>
          <div class="brand-sub">WebDAV → 本地磁盘</div>
        </div>
      </div>

      <div class="header-meta">
        <span v-if="settings" class="badge" :class="settings.rcloneFound ? 'muted' : 'err'">
          <span class="dot" />
          rclone {{ settings.rcloneFound ? settings.rcloneVersion : '未找到' }}
        </span>
        <span v-if="settings && settings.winfspRequired" class="badge" :class="settings.winfspOk ? 'ok' : 'err'">
          <span class="dot" />
          WinFsp {{ settings.winfspOk ? '已安装' : '未安装' }}
        </span>
        <button v-if="settings" class="btn sm" :class="settings.autoRemount ? 'primary' : ''" @click="toggleAutoRemount">
          掉线重挂 {{ settings.autoRemount ? '开' : '关' }}
        </button>
        <button v-if="settings" class="btn sm" :class="settings.autostart ? 'primary' : ''" @click="toggleAutostart">
          开机自启 {{ settings.autostart ? '开' : '关' }}
        </button>
        <button class="btn sm ghost" @click="openDataDir">数据目录</button>
      </div>
    </header>

    <div v-if="winfspWarning" style="padding: 0 18px; margin-top: 12px">
      <div class="badge err" style="width: 100%; border-radius: 10px; padding: 9px 12px; white-space: normal">
        <span class="dot" />
        Windows 上挂载需要先安装 WinFsp（免费开源）：https://winfsp.dev/rel/ —— 装完重启本程序即可。
      </div>
    </div>

    <main class="app-main">
      <section class="col">
        <div class="col-head">
          <span class="col-title">挂载项</span>
          <span class="badge muted">{{ remotes.length }}</span>
          <span style="flex: 1" />
          <button class="btn primary sm" @click="startNew">＋ 新建</button>
        </div>

        <div v-if="remotes.length === 0" class="empty">
          还没有挂载项。点右上角「新建」，填上 WebDAV 地址和挂载点就能用了。
        </div>

        <article
          v-for="remote in remotes"
          :key="remote.name"
          class="card remote-card"
          :class="{ active: editing?.name === remote.name }"
        >
          <div class="remote-top">
            <span class="badge" :class="stateClass(remote.name)">
              <span class="dot" />
              {{ stateText(remote.name) }}
            </span>
            <span class="remote-name">{{ remote.name }}</span>
            <span style="flex: 1" />
            <span v-if="statusOf(remote.name)?.pid" class="badge muted">PID {{ statusOf(remote.name)?.pid }}</span>
          </div>

          <div class="remote-line">
            <code>{{ remote.mountPoint }}</code>
            <span>←</span>
            <code>{{ remote.name }}:{{ remote.subPath || '' }}</code>
          </div>
          <div class="remote-line" style="font-size: 11.5px">
            {{ remote.url }}
          </div>
          <div v-if="statusOf(remote.name)?.error" class="remote-error">
            {{ statusOf(remote.name)?.error }}
          </div>

          <div class="remote-actions">
            <button
              v-if="statusOf(remote.name)?.state !== 'mounted'"
              class="btn primary sm"
              :disabled="busy === remote.name"
              @click="mount(remote.name)"
            >
              <span v-if="busy === remote.name" class="spin" />
              {{ busy === remote.name ? '挂载中…' : '挂载' }}
            </button>
            <button
              v-else
              class="btn sm"
              :disabled="busy === remote.name"
              @click="unmount(remote.name)"
            >
              <span v-if="busy === remote.name" class="spin" />
              卸载
            </button>
            <button class="btn sm ghost" @click="startEdit(remote)">编辑</button>
            <button class="btn sm ghost" @click="openLog(remote.name)">日志</button>
            <span v-if="remote.autoMount" class="badge brand">自启挂载</span>
            <span style="flex: 1" />
            <button class="btn sm danger" :disabled="busy === remote.name" @click="askDelete(remote.name)">
              {{ pendingDelete === remote.name ? '确认删除' : '删除' }}
            </button>
          </div>
        </article>
      </section>

      <section class="col">
        <RemoteForm
          v-if="editing"
          :model-value="editing"
          :is-new="isNew"
          :platform="platform"
          :saving="saving"
          @submit="save"
          @cancel="editing = null"
        />
        <div v-else class="card form">
          <div class="form-title">说明</div>
          <div class="hint">
            挂载由 rclone 完成，本程序只负责配置与进程守护；关掉窗口会缩到托盘，挂载继续。
          </div>
          <div class="hint">
            开机自启写入当前用户的启动项（HKCU Run），不会申请管理员权限。
          </div>
          <div class="hint">
            掉线重挂开启后，rclone 进程意外退出会被自动拉起，最多连续 3 次。
          </div>
          <div class="hint">日志在 <code>{{ settings?.dataDir }}\logs</code>，也可以点卡片上的「日志」直接看尾部。</div>
        </div>
      </section>
    </main>

    <footer class="statusbar">
      <span v-if="message" class="msg" :class="message.kind">
        <span class="dot" />
        {{ message.text }}
      </span>
      <span v-else class="msg info">就绪</span>
      <span class="spacer" />
      <span v-if="settings" class="msg info">{{ settings.rclonePath }}</span>
      <button class="btn sm ghost" @click="api.hideWindow()">隐藏到托盘</button>
    </footer>

    <div v-if="logName" class="drawer">
      <div class="drawer-head">
        <span>日志 · {{ logName }}</span>
        <span class="spacer" />
        <button class="btn sm ghost" @click="loadLog">刷新</button>
        <button class="btn sm ghost" @click="closeLog">关闭</button>
      </div>
      <pre class="log-body">{{ logText || '（暂无日志）' }}</pre>
    </div>
  </div>
</template>