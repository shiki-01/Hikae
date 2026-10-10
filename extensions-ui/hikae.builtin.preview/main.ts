// 組み込みの表示拡張（iframe の UI）の入口。実際の表示は `#lib/ext-ui` の薄いシェルが行う（設計書 11 章）
import { mount } from 'svelte';
import '#lib/styles/tokens.css';
import RendererApp from '#lib/ext-ui/RendererApp.svelte';

const target = document.getElementById('app');
if (target) {
	mount(RendererApp, { target });
}
