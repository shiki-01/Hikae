<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import TimelineItem from '#lib/components/TimelineItem.svelte';
	import type { TimelineEntry } from '#lib/components/timeline.js';

	interface Props {
		entries: TimelineEntry[];
		loading: boolean;
		/** まだ読み込んでいない、より古い履歴がある */
		hasMore?: boolean;
		/** 古い履歴を読み込んでいる */
		loadingMore?: boolean;
		/** 古い履歴の読み込みに失敗した（自動では繰り返さず、「もう一度」を出す） */
		loadMoreFailed?: boolean;
		/** 末尾に近づいたとき（または「もう一度」を押したとき）に、次のページを読み込む */
		onloadmore?: () => void;
		selectedId: string | null;
		expanded: string[];
		/** 自動保存を常に開いて表示するか */
		showAutos?: boolean;
		onselect: (id: string) => void;
		ontoggle: (id: string) => void;
	}

	let {
		entries,
		loading,
		hasMore = false,
		loadingMore = false,
		loadMoreFailed = false,
		onloadmore,
		selectedId,
		expanded,
		showAutos = false,
		onselect,
		ontoggle
	}: Props = $props();

	const saveCount = $derived(entries.filter((e) => e.kind === 'save').length);

	let scroller = $state<HTMLDivElement>();
	let sentinel = $state<HTMLElement>();

	// 末尾の目印が見える範囲に入ったら、次のページを読み込む。
	// 読み込みのたびに見張りを作り直すので、読み込んだ後も目印が見えたままなら続けて読み込む
	$effect(() => {
		if (!sentinel || !scroller || !hasMore || loadingMore || loadMoreFailed) return;
		void entries.length;
		if (typeof IntersectionObserver === 'undefined') return;
		const observer = new IntersectionObserver(
			(records) => {
				if (records.some((record) => record.isIntersecting)) onloadmore?.();
			},
			{ root: scroller, rootMargin: '0px 0px 200px 0px' }
		);
		observer.observe(sentinel);
		return () => observer.disconnect();
	});
</script>

<div bind:this={scroller} class="flex:1 min-h:0 min-w:0 overflow-y:auto overflow-x:hidden p:3">
	{#if loading}
		<div class="flex flex-direction:column gap:3" aria-busy="true">
			{#each [0, 1, 2, 3] as row (row)}
				<Skeleton class="h:48px" />
			{/each}
		</div>
	{:else}
		<ol class="m:0 p:0 list-style:none" aria-label={t('history.list_label')}>
			{#each entries as entry, index (entry.id)}
				<TimelineItem
					{entry}
					{selectedId}
					expanded={showAutos || expanded.includes(entry.id)}
					isLast={index === entries.length - 1}
					{onselect}
					{ontoggle}
				/>
			{/each}
		</ol>
		{#if hasMore}
			<div
				bind:this={sentinel}
				class="flex align-items:center justify-content:center gap:2 py:3"
				data-testid="history-sentinel"
			>
				{#if loadMoreFailed}
					<Button size="sm" variant="secondary" onclick={onloadmore}>
						{t('history.load_more_retry')}
					</Button>
				{:else}
					<Spinner size="sm" label={t('history.loading_more')} />
					<span class="type-small fg:fg-muted" aria-hidden="true">{t('history.loading_more')}</span>
				{/if}
			</div>
		{/if}
		{#if saveCount <= 1}
			<p class="m:0 mt:3 type-small fg:fg-muted text-align:center">{t('timeline.empty_message')}</p>
		{/if}
	{/if}
</div>
