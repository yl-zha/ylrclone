<script setup lang="ts">
import { reactive, ref, watch } from 'vue'

import type { RemoteSpec } from '../api'

const props = defineProps<{
  modelValue: RemoteSpec | null
  isNew: boolean
  platform: string
  saving: boolean
}>()

const emit = defineEmits<{
  submit: [spec: RemoteSpec, pass: string | null]
  cancel: []
}>()

function blank(): RemoteSpec {
  return {
    name: '',
    url: '',
    vendor: '',
    user: '',
    mountPoint: props.platform === 'windows' ? 'X:' : '/mnt/remote',
    subPath: '',
    extraFlags: [],
    autoMount: false,
    networkMode: false,
  }
}

const form = reactive<RemoteSpec>(blank())
const pass = ref('')
const flagsText = ref('')
const error = ref('')

function reset(spec: RemoteSpec | null) {
  Object.assign(form, spec ? { ...spec, extraFlags: [...spec.extraFlags] } : blank())
  pass.value = ''
  flagsText.value = (form.extraFlags || []).join('\n')
  error.value = ''
}

watch(() => props.modelValue, (spec) => reset(spec), { immediate: true })

function applyDrive(letter: string) {
  form.mountPoint = `${letter}:`
}

function submit() {
  error.value = ''
  const name = form.name.trim()
  if (!name) return (error.value = '名称不能为空')
  if (/[:\\/]/.test(name)) return (error.value = '名称不能包含 : / \\ 这三个字符')
  if (!form.url.trim()) return (error.value = 'WebDAV 地址不能为空')
  if (!form.mountPoint.trim()) return (error.value = '挂载点不能为空')

  const extraFlags = flagsText.value
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    // --network-mode 由勾选框统一管理，避免重复或互相覆盖
    .filter((line) => line !== '--network-mode' && !line.startsWith('--network-mode='))

  emit(
    'submit',
    {
      ...form,
      name,
      url: form.url.trim(),
      user: form.user.trim(),
      mountPoint: form.mountPoint.trim(),
      subPath: form.subPath.trim(),
      extraFlags,
    },
    pass.value === '' ? null : pass.value,
  )
}
</script>

<template>
  <div class="card form">
    <div class="form-title">
      <span>{{ isNew ? '新建 WebDAV 挂载' : `编辑 ${form.name}` }}</span>
      <span class="spacer" style="flex: 1" />
    </div>

    <div class="field">
      <label>名称（rclone remote 名）</label>
      <input
        v-model="form.name"
        class="input"
        :disabled="!isNew"
        placeholder="例如 115dav"
        spellcheck="false"
      />
      <span v-if="isNew" class="hint">只用字母、数字、下划线、短横线；挂载后它也是 rclone.conf 里的段名。</span>
    </div>

    <div class="field">
      <label>WebDAV 地址</label>
      <input v-model="form.url" class="input" spellcheck="false" placeholder="http://127.0.0.1:5244/dav" />
    </div>

    <div class="row">
      <div class="field">
        <label>用户名</label>
        <input v-model="form.user" class="input" spellcheck="false" autocomplete="off" />
      </div>
      <div class="field">
        <label>密码</label>
        <input
          v-model="pass"
          class="input"
          type="password"
          autocomplete="new-password"
          :placeholder="isNew ? '保存时用 rclone obscure 加密' : '留空表示不修改'"
        />
      </div>
    </div>

    <div class="field">
      <label>挂载点</label>
      <div class="row">
        <input
          v-model="form.mountPoint"
          class="input"
          spellcheck="false"
          :placeholder="platform === 'windows' ? 'X: 或 D:\\Mount\\dav' : '/mnt/remote'"
        />
      </div>
      <div v-if="platform === 'windows'" class="remote-actions" style="margin-top: 2px">
        <button
          v-for="letter in ['X', 'Y', 'Z', 'M', 'N']"
          :key="letter"
          type="button"
          class="btn sm ghost"
          @click="applyDrive(letter)"
        >
          {{ letter }}:
        </button>
        <span class="hint">盘符不能被占用；目录挂载需目录已存在或用不存在的空路径。</span>
      </div>
    </div>

    <div class="field">
      <label>远程子路径（可空）</label>
      <input v-model="form.subPath" class="input" spellcheck="false" placeholder="留空挂载根目录，例如 sub/dir" />
    </div>

    <div class="field">
      <label>WebDAV 类型（vendor）</label>
      <select v-model="form.vendor" class="select">
        <option value="">默认（other）</option>
        <option value="other">other</option>
        <option value="nextcloud">nextcloud</option>
        <option value="owncloud">owncloud</option>
        <option value="sharepoint">sharepoint</option>
      </select>
    </div>

    <label class="check">
      <input v-model="form.autoMount" type="checkbox" />
      启动时自动挂载（配合托盘常驻）
    </label>

    <label v-if="platform === 'windows'" class="check">
      <input v-model="form.networkMode" type="checkbox" />
      网络模式（--network-mode，用 \\server\\share 而不占盘符）
    </label>

    <div class="field">
      <label>额外 rclone 参数（每行一个）</label>
      <textarea
        v-model="flagsText"
        class="textarea"
        spellcheck="false"
        placeholder="--dir-cache-time=5m&#10;--buffer-size=32M&#10;--vfs-cache-max-size=10G"
      />
      <span class="hint">默认会补 --vfs-cache-mode=writes；想要只读可加 --read-only。</span>
    </div>

    <div v-if="error" class="remote-error">{{ error }}</div>

    <div class="form-actions">
      <button class="btn primary" :disabled="saving" @click="submit">
        <span v-if="saving" class="spin" />
        {{ saving ? '保存中…' : '保存' }}
      </button>
      <button class="btn ghost" :disabled="saving" @click="emit('cancel')">取消</button>
    </div>
  </div>
</template>