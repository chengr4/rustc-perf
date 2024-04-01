<script setup lang="tsx">
import {computed, ref, Ref} from "vue";
import {CompareResponse, Tab} from "../pages/compare/types";
import {SummaryGroup} from "../pages/compare/data";
import TabComponent from "./tab.vue";

const props = withDefaults(
  defineProps<{
    data: CompareResponse;
    compileTimeSummary: SummaryGroup;
    runtimeSummary: SummaryGroup;
    initialTab?: Tab;
    tabs;
  }>(),
  {
    initialTab: Tab.CompileTime,
  }
);
const emit = defineEmits<{
  (e: "changeTab", tab: Tab): void;
}>();

function changeTab(tab: Tab) {
  activeTab.value = tab;
  emit("changeTab", tab);
}
const activeTab: Ref<Tab> = ref(props.initialTab);
const activeTabs = computed(() => {
  return props.tabs.filter((tab) => tab.isVisible)
})
</script>

<template>
  <div class="wrapper">
    <TabComponent
      v-for="tab in activeTabs"
      :key="tab.id"
      :tooltip="tab.tooltip"
      :title="tab.title"
      :selected="tab.selected"
      @click="changeTab(tab.id)"
    >
      <template v-slot:summary>
        <component :is="tab.summary"></component>
      </template>
    </TabComponent>
  </div>
</template>

<style scoped lang="scss">
.wrapper {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 20px 0;

  @media (min-width: 600px) {
    justify-content: center;
    flex-direction: row;
    align-items: normal;
  }
}
</style>
