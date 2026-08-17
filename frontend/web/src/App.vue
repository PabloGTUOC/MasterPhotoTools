<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { LayoutDashboard, Library, Wrench, Settings as SettingsIcon } from 'lucide-vue-next'

const router = useRouter()
const activePath = ref('/')

const navItems = [
  { path: '/', label: 'Dashboard', icon: LayoutDashboard },
  { path: '/library', label: 'Library', icon: Library },
  { path: '/f1', label: 'Date Repair', icon: Wrench },
]

router.afterEach((to) => {
  activePath.value = to.path
})

const navigate = (path: string) => {
  router.push(path)
}
</script>

<template>
  <div class="app-layout">
    <aside class="sidebar">
      <div class="sidebar-header">
        <h2>PhotoTools</h2>
      </div>
      <nav class="sidebar-nav">
        <button 
          v-for="item in navItems" 
          :key="item.path"
          :class="['nav-item', activePath === item.path ? 'active' : '']"
          @click="navigate(item.path)"
        >
          <component :is="item.icon" :size="18" />
          {{ item.label }}
        </button>
        <div class="spacer"></div>
      </nav>
    </aside>
    
    <main class="main-content">
      <router-view></router-view>
    </main>
    
    <footer class="status-bar">
      <span>Ready</span>
      <span>Version 0.1.0</span>
    </footer>
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  flex-direction: row;
  height: 100vh;
  width: 100vw;
  position: relative;
}

.sidebar {
  width: 260px;
  background-color: #1a1a1a;
  border-right: 1px solid #333;
  display: flex;
  flex-direction: column;
}

.sidebar-header {
  padding: 24px;
  color: #fff;
  border-bottom: 1px solid #333;
}

.sidebar-nav {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
}

.spacer {
  flex: 1;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 6px;
  color: #a0a0a0;
  background: transparent;
  border: none;
  cursor: pointer;
  text-align: left;
  font-size: 0.9rem;
  font-weight: 500;
  transition: all 0.2s ease;
}

.nav-item:hover {
  background-color: #2a2a2a;
  color: #fff;
}

.nav-item.active {
  background-color: #3b82f6;
  color: #fff;
}

.main-content {
  flex: 1;
  background-color: #121212;
  overflow-y: auto;
  padding: 24px;
  padding-bottom: 50px;
}

.status-bar {
  position: absolute;
  bottom: 0;
  left: 260px;
  right: 0;
  height: 32px;
  background-color: #1a1a1a;
  border-top: 1px solid #333;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  font-size: 0.75rem;
  color: #a0a0a0;
}

/* Mobile responsive (for "mobile-first" approach requested in Phase 6) */
@media (max-width: 768px) {
  .app-layout {
    flex-direction: column;
  }
  
  .sidebar {
    width: 100%;
    height: auto;
    border-right: none;
    border-bottom: 1px solid #333;
  }
  
  .status-bar {
    left: 0;
  }
}
</style>
