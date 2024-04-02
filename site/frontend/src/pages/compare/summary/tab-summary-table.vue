<script setup lang="ts">
import {SummaryGroup} from "../data";
import {computed} from "vue";
import SummaryRange from "./range.vue";
// import SummaryCount from "./count.vue";
import SummaryPercentValue from "./percent-value.vue";
import {
  percentClass,
  // diffClass,
  // formatPercentChange,
  // formatSize,
} from "../shared";

const props = defineProps<{
  summary: SummaryGroup;
}>();
const summary = computed(() => props.summary);
</script>
<template>
  <div v-if="summary.all.count > 0">
    <div class="table-wrapper">
      <table>
        <thead>
          <tr>
            <th>Range</th>
            <th>Mean</th>
          </tr>
        </thead>
        <thead>
          <tr>
            <td>
              <SummaryRange :range="summary.all.range" />
            </td>
            <td>
              <SummaryPercentValue
                :class="percentClass(summary.all.average)"
                :value="summary.all.average"
              />
            </td>
          </tr>
        </thead>
      </table>
    </div>
  </div>
  <div v-else>
    <span>No results</span>
  </div>
</template>

<style scoped lang="scss">
.table-wrapper {
  table {
    width: 100%;
    table-layout: auto;
  }

  th {
    font-weight: normal;
  }
}
</style>