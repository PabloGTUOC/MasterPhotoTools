import { createApp } from 'vue'
import { createRouter, createWebHistory } from 'vue-router'
import './style.css'
import App from './App.vue'

// Import components that we'll create shortly
import F1Dates from './components/F1Dates.vue'
import Dashboard from './components/Dashboard.vue'
import LibraryBrowser from './components/LibraryBrowser.vue'

const routes = [
  { path: '/', component: Dashboard },
  { path: '/library', component: LibraryBrowser },
  { path: '/f1', component: F1Dates },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

const app = createApp(App)
app.use(router)
app.mount('#app')
