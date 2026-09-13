<script setup lang="ts">
import {ref,onMounted,onBeforeUnmount,computed,nextTick} from 'vue';
import {Download,WandSparkles,Settings2,Plus,Files,X} from '@lucide/vue';
import {listen} from '@tauri-apps/api/event';
import TranscodePanel from './components/TranscodePanel.vue';
import DownloadPanel from './components/DownloadPanel.vue';import TaskList from './components/TaskList.vue';import SettingsPanel from './components/SettingsPanel.vue';
import {desktop,initialize,settings,snapshot,command,addTasks,showError,chooseDirectory,error,message,messagePage,activePage} from './store';
const page=activePage;const settingsPanel=ref<InstanceType<typeof SettingsPanel>>();const transcodePanel=ref<InstanceType<typeof TranscodePanel>>();let disposed=false;let cleanup=()=>{};let unlisten=()=>{};
const heading=computed(()=>({download:'下载',transcode:'转码',settings:'设置'}[page.value]));
const subtitle=computed(()=>({download:'把喜欢的视频，留在本地。',transcode:'按画质或目标大小，批量转换视频。',settings:'默认设置应用于之后添加的新任务。'}[page.value]));
function addPaths(paths:string[]){void transcodePanel.value?.addPaths(paths)}
async function chooseFiles(){await transcodePanel.value?.chooseFiles()}
async function openNetwork(){page.value='settings';await nextTick();if(settingsPanel.value)settingsPanel.value.tab='network'}
onMounted(async()=>{try{const stop=await initialize();if(disposed){stop();return}cleanup=stop;if(desktop){const drop=await listen<{paths:string[]}>('tauri://drag-drop',e=>{if(page.value==='transcode')addPaths(e.payload.paths)});if(disposed)drop();else unlisten=drop}}catch(e){showError(e)}});onBeforeUnmount(()=>{disposed=true;cleanup();unlisten()});
</script>
<template><div class="app-shell"><aside class="sidebar"><div class="brand"><span class="brand-icon"><Download :size="20"/></span><strong>视频下载器</strong></div><nav aria-label="主导航"><button v-for="item in [{id:'download',name:'下载',icon:Download},{id:'transcode',name:'转码',icon:WandSparkles},{id:'settings',name:'设置',icon:Settings2}]" :key="item.id" :class="{active:page===item.id}" @click="page=item.id"><component :is="item.icon" :size="19"/><span>{{item.name}}</span></button></nav><div class="version">视频下载器<br>v0.4.2 <span v-if="!desktop">· 网页预览</span></div></aside><main><header class="page-header"><div><h1>{{heading}}</h1><p>{{subtitle}}</p></div><button class="button quiet" v-if="page==='download'" @click="openNetwork"><span class="network-dot" :class="{off:!settings.use_proxy}"></span>{{settings.use_proxy?'使用代理':'直连模式'}}</button><button class="button primary" v-if="page==='transcode'" @click="chooseFiles"><Plus :size="16"/>添加文件</button></header>
<div v-if="error||snapshot.persistence_error" class="alert error" role="alert"><span class="break">{{error||snapshot.persistence_error}}</span><button class="icon-button" aria-label="关闭错误提示" @click="error=''"><X :size="17"/></button></div>
<section v-show="page==='download'"><DownloadPanel/><TaskList kind="download"/></section>
<section v-show="page==='transcode'"><TranscodePanel ref="transcodePanel"/><TaskList kind="transcode"/></section>
<section v-show="page==='settings'"><SettingsPanel ref="settingsPanel"/></section>
</main><div v-if="message&&messagePage===page" class="alert success bottom-notice" aria-live="polite" aria-atomic="true" role="status"><span>{{message}}</span><button class="icon-button" aria-label="关闭提示" @click="message=''"><X :size="17"/></button></div></div></template>
