<script lang="ts">
	import { onMount } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import { autoSaveView } from './auto-save-status';

	interface Props {
		/** 自動保存が有効か（そのプロジェクトで実際に使う設定） */
		enabled: boolean;
		/** 最後の変更から、この秒数だけ静止すると自動保存が作られる */
		delaySecs: number;
		/** 最後に自動保存が作られた時刻。まだ無ければ null */
		lastAt: Date | null;
		onsettings: () => void;
	}

	let { enabled, delaySecs, lastAt, onsettings }: Props = $props();

	// 「3 分前」の表示が古くならないよう、一定間隔で現在時刻を更新する
	let now = $state(new Date());
	onMount(() => {
		const timer = setInterval(() => (now = new Date()), 30_000);
		return () => clearInterval(timer);
	});

	const view = $derived(autoSaveView(enabled, delaySecs, lastAt, now));
</script>

<div class="flex flex-direction:column gap:1 px:3 py:2 bb:1px|solid|border flex-shrink:0">
	{#if view.kind === 'on'}
		<p class="m:0 type-small fg:fg-muted">{view.rule}</p>
		<p class="m:0 type-small fg:fg-muted">{view.last}</p>
	{:else}
		<p class="m:0 type-small fg:fg-muted">
			{t('autosave.off')}
			<button
				type="button"
				class="p:0 b:0 bg:transparent fg:accent type-small cursor:pointer text-decoration:underline"
				onclick={onsettings}
			>
				{t('autosave.open_settings')}
			</button>
		</p>
	{/if}
</div>
