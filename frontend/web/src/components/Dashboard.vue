<script setup lang="ts">
import { ref } from 'vue'
</script>

<template>
  <div class="dashboard">
    <header class="page-header">
      <h1>Dashboard</h1>
    </header>
    <div class="card">
      <p>Welcome to PhotoTools.</p>
      <p class="text-muted">Select an archive tool from the menu to get started.</p>
    </div>
  </div>
</template>

<style scoped>
.page-header h1 {
  font-size: 1.75rem;
  margin-bottom: 24px;
  color: #fff;
}
.card {
  background-color: #1a1a1a;
  border: 1px solid #333;
  border-radius: 8px;
  padding: 24px;
  color: #e0e0e0;
}
.text-muted {
  color: #a0a0a0;
  margin-top: 8px;
}
</style>
