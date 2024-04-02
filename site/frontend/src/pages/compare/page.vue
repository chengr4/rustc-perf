<script setup lang="tsx">
import {loadBenchmarkInfo} from "../../api";
import AsOf from "../../components/as-of.vue";
import {
  changeUrl,
  createUrlWithAppendedParams,
  getUrlParams,
  navigateToUrlParams,
} from "../../utils/navigation";
import {computed, Ref, ref, h} from "vue";
import {withLoading} from "../../utils/loading";
import {postMsgpack} from "../../utils/requests";
import {COMPARE_DATA_URL} from "../../urls";
import {CompareResponse, CompareSelector, Tab} from "./types";
import BootstrapTable from "./bootstrap/bootstrap-table.vue";
import Header from "./header/header.vue";
import DataSelector, {SelectionParams} from "./header/data-selector.vue";
import {computeSummary, filterNonRelevant, SummaryGroup} from "./data";
import Tabs from "../../components/tabs.vue";
import CompileBenchmarksPage from "./compile/compile-page.vue";
import {
  computeCompileComparisonsWithNonRelevant,
  createCompileBenchmarkMap,
  defaultCompileFilter as defaultCompileFilter,
} from "./compile/common";
import RuntimeBenchmarksPage from "./runtime/runtime-page.vue";
import {
  computeRuntimeComparisonsWithNonRelevant,
  defaultRuntimeFilter,
} from "./runtime/common";
import ArtifactSizeTable from "./artifact-size/artifact-size-table.vue";
import TabSummaryTable from "./summary/tab-summary-table.vue";

// ------ block abstract from tabs.vue ------
import {
  diffClass,
  formatPercentChange,
  formatSize,
} from "./shared";
// ------ block abstract from tabs.vue ------

function loadSelectorFromUrl(urlParams: Dict<string>): CompareSelector {
  const start = urlParams["start"] ?? "";
  const end = urlParams["end"] ?? "";
  const stat = urlParams["stat"] ?? "instructions:u";
  return {
    start,
    end,
    stat,
  };
}

function loadTabFromUrl(urlParams: Dict<string>): Tab | null {
  const tab = urlParams["tab"] ?? "";
  const tabs = {
    compile: Tab.CompileTime,
    runtime: Tab.Runtime,
    bootstrap: Tab.Bootstrap,
    ["artifact-size"]: Tab.ArtifactSize,
  };
  return tabs[tab] ?? null;
}

function storeTabToUrl(urlParams: Dict<string>, tab: Tab) {
  urlParams["tab"] = tab as string;
  changeUrl(urlParams);
}

async function loadCompareData(
  selector: CompareSelector,
  loading: Ref<boolean>
) {
  const response = await withLoading(loading, async () => {
    const params = {
      start: selector.start,
      end: selector.end,
      stat: selector.stat,
    };
    return await postMsgpack<CompareResponse>(COMPARE_DATA_URL, params);
  });
  data.value = response;

  compileSummary.value = computeSummary(
    filterNonRelevant(
      defaultCompileFilter,
      computeCompileComparisonsWithNonRelevant(
        defaultCompileFilter,
        response.compile_comparisons,
        createCompileBenchmarkMap(response)
      )
    )
  );
  runtimeSummary.value = computeSummary(
    filterNonRelevant(
      defaultRuntimeFilter,
      computeRuntimeComparisonsWithNonRelevant(
        defaultRuntimeFilter,
        response.runtime_comparisons
      )
    )
  );
}

function updateSelection(params: SelectionParams) {
  navigateToUrlParams(
    createUrlWithAppendedParams({
      start: params.start,
      end: params.end,
      stat: params.stat,
    }).searchParams
  );
}

const urlParams = getUrlParams();

// Include all relevant changes in the compile-time and runtime tab summaries.
// We do not wrap these summaries in `computed`, because they should be loaded
// only once, after the compare data is downloaded.
const compileSummary: Ref<SummaryGroup | null> = ref(null);
const runtimeSummary: Ref<SummaryGroup | null> = ref(null);

const loading = ref(false);

const selector = loadSelectorFromUrl(urlParams);

const initialTab: Tab = loadTabFromUrl(urlParams) ?? Tab.CompileTime;
const tab: Ref<Tab> = ref(initialTab);
const activeTab = computed((): Tab => {
  if (tab.value === Tab.ArtifactSize && !artifactSizeAvailable.value) {
    return Tab.CompileTime;
  }
  return tab.value;
});

const artifactSizeAvailable = computed(
  () =>
    data.value != null &&
    (Object.keys(data.value.a.component_sizes).length > 0 ||
      Object.keys(data.value.b.component_sizes).length > 0)
);

function changeTab(newTab: Tab) {
  tab.value = newTab;
  storeTabToUrl(getUrlParams(), newTab);
}

const data: Ref<CompareResponse | null> = ref(null);
loadCompareData(selector, loading);
let info = await loadBenchmarkInfo();

// ------ block abstract from tabs.vue ------
function formatBootstrap(value: number): string {
  if (value > 0.0) {
    return (value / 10e8).toFixed(3);
  }
  return "???";
}

function formatArtifactSize(size: number): string {
  if (size === 0) {
    return "???";
  }
  return formatSize(size);
}

const bootstrapA = data.value.a.bootstrap_total;
const bootstrapB = data.value.b.bootstrap_total;
const bootstrapValid = bootstrapA > 0.0 && bootstrapB > 0.0;

const totalSizeA = Object.values(data.value.a.component_sizes).reduce(
  (a, b) => a + b,
  0
);
const totalSizeB = Object.values(data.value.b.component_sizes).reduce(
  (a, b) => a + b,
  0
);
const sizesAvailable: boolean = totalSizeA > 0 || totalSizeB > 0;
const bothSizesAvailable: boolean = totalSizeA > 0 && totalSizeB > 0;

const tabs = [
  {
    tooltip:
      "Compilation time benchmarks: measure how long does it take to compile various crates using the compared rustc.",
    title: "Compile-time",
    selected: activeTab.value === Tab.CompileTime,
    id: Tab.CompileTime,
    summary: <TabSummaryTable summary={compileSummary.value} />,
    isVisible: true,
  },
  {
    tooltip:
      "Runtime benchmarks: measure how long does it take to execute (i.e. how fast are) programs compiled by the compared rustc.",
    title: "Runtime",
    selected: activeTab.value === Tab.Runtime,
    id: Tab.Runtime,
    summary: <TabSummaryTable summary={runtimeSummary.value} />,
    isVisible: true,
  },
  {
    tooltip:
      "Bootstrap duration: measures how long does it take to compile rustc by itself.",
    title: "Bootstrap",
    selected: activeTab.value === Tab.Bootstrap,
    id: Tab.Bootstrap,
    summary: (
      <div>
        {formatBootstrap(bootstrapA)} {"->"} {formatBootstrap(bootstrapB)}
        {bootstrapValid && (
          <div class={diffClass(bootstrapB - bootstrapA)}>
            {((bootstrapB - bootstrapA) / 10e8).toFixed(1)}s (
            {(((bootstrapB - bootstrapA) / bootstrapA) * 100).toFixed(3)}
            %)
          </div>
        )}
      </div>
    ),
    isVisible: true,
  },
  {
    tooltip:
      "Artifact size: sizes of individual components of the two artifacts.",
    title: "Artifact size",
    selected: activeTab.value === Tab.ArtifactSize,
    id: Tab.ArtifactSize,
    summary: (
      <div>
        {formatArtifactSize(totalSizeA)} {"->"} {formatArtifactSize(totalSizeB)}
        {bothSizesAvailable && (
          <div class={diffClass(totalSizeB - totalSizeA)}>
            {totalSizeB < totalSizeA ? "-" : ""}
            {formatSize(Math.abs(totalSizeB - totalSizeA))} (
            {formatPercentChange(totalSizeA, totalSizeB)})
          </div>
        )}
      </div>
    ),
    isVisible: sizesAvailable,
  },
];
// ------ block abstract from tabs.vue ------
</script>

<template>
  <div>
    <Header :data="data" :selector="selector" />
    <DataSelector
      :start="selector.start"
      :end="selector.end"
      :stat="selector.stat"
      :info="info"
      @change="updateSelection"
    />
    <div v-if="loading">
      <p>Loading ...</p>
    </div>
    <div v-if="data !== null">
      <Tabs
        @change-tab="changeTab"
        :data="data"
        :initial-tab="initialTab"
        :compile-time-summary="compileSummary"
        :runtime-summary="runtimeSummary"
        :tabs="tabs"
      />
      <template v-if="activeTab === Tab.CompileTime">
        <CompileBenchmarksPage
          :data="data"
          :selector="selector"
          :benchmark-info="info"
        />
      </template>
      <template v-if="activeTab === Tab.Runtime">
        <RuntimeBenchmarksPage
          :data="data"
          :selector="selector"
          :benchmark-info="info"
        />
      </template>
      <BootstrapTable v-if="activeTab === Tab.Bootstrap" :data="data" />
      <template v-if="artifactSizeAvailable && activeTab === Tab.ArtifactSize">
        <ArtifactSizeTable :a="data.a" :b="data.b" />
      </template>
    </div>
  </div>
  <br />
  <AsOf :info="info" />
</template>
