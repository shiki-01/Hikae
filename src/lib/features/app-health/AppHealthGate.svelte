<script lang="ts">
	import type { Snippet } from 'svelte';
	import { t } from '#lib/i18n/index.js';
	import Spinner from '#lib/components/Spinner.svelte';
	import GitUnavailableScreen from './GitUnavailableScreen.svelte';
	import { useAppHealth } from './queries';

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();

	const health = useAppHealth();
</script>

<!--
	保存に必要な部品が使えないとき（E17）は、プロジェクトの画面へ進ませず、専用の案内を出す。
	確認そのものに失敗したとき（通信の失敗ではなく、確認の呼び出しの失敗）は、画面を止めずに通常どおり進める
-->
{#if health.isPending}
	<div class="h:100vh flex align-items:center justify-content:center bg:bg fg:fg-muted gap:2">
		<Spinner />
		<span class="type-small">{t('app_health.checking')}</span>
	</div>
{:else if health.data && !health.data.partsAvailable}
	<GitUnavailableScreen
		health={health.data}
		checking={health.isFetching}
		onretry={() => void health.refetch()}
	/>
{:else}
	{@render children()}
{/if}
