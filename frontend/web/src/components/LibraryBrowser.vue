<script setup lang="ts">
import { ref } from 'vue'

const currentPath = ref('/Volumes/PhotoArchive')

// In a real implementation this would fetch from the shared API
const items = ref([
  { name: '2019', type: 'directory' },
  { name: '2020', type: 'directory' },
  { name: '2021', type: 'directory' },
  { name: '2022', type: 'directory' },
])

const navigateTo = (folderName: string) => {
  currentPath.value = currentPath.value + '/' + folderName
  // mock fetch...
  items.value = [
    { name: '01-January', type: 'directory' },
    { name: '02-February', type: 'directory' },
  ]
}

const navigateUp = () => {
  const parts = currentPath.value.split('/')
  if (parts.length > 2) {
    parts.pop()
    currentPath.value = parts.join('/')
  }
}
</script>

<template>
  <div class="library-browser">
    <header class="page-header">
      <h1>Library Browser</h1>
    </header>
    
    <div class="browser-card">
      <div class="breadcrumb">
        <button class="btn-up" @click="navigateUp">↑ Up</button>
        <span class="path">{{ currentPath }}</span>
      </div>
      
      <div class="file-list">
        <div 
          v-for="item in items" 
          :key="item.name" 
          class="file-item"
          @click="item.type === 'directory' ? navigateTo(item.name) : null"
        >
          <span class="icon">{{ item.type === 'directory' ? '📁' : '📄' }}</span>
          <span class="name">{{ item.name }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.page-header h1 {
  font-size: 1.75rem;
  margin-bottom: 24px;
  color: #fff;
}
.browser-card {
  background-color: #1a1a1a;
  border: 1px solid #333;
  border-radius: 8px;
  overflow: hidden;
}
.breadcrumb {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 16px;
  background-color: #222;
  border-bottom: 1px solid #333;
}
.btn-up {
  background-color: #333;
  border: none;
  color: #fff;
  padding: 6px 12px;
  border-radius: 4px;
  cursor: pointer;
}
.btn-up:hover {
  background-color: #444;
}
.path {
  font-family: monospace;
  color: #e0e0e0;
}
.file-list {
  padding: 8px;
}
.file-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  color: #e0e0e0;
  cursor: pointer;
  border-radius: 6px;
}
.file-item:hover {
  background-color: #2a2a2a;
}
.icon {
  font-size: 1.25rem;
}
</style>
