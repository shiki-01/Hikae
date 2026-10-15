<script lang="ts">
	import type { AiModel } from '#lib/api/types.js';
	import { t } from '#lib/i18n/index.js';
	import {
		formatBytes,
		modelButtons,
		progressPercent,
		recommendationKey,
		stageLevelKey,
		stageNoteKey,
		unavailableMessage,
		type ModelAction
	} from '#lib/features/ai/model-view.js';
	import Badge from './Badge.svelte';
	import Button from './Button.svelte';
	import ProgressBar from './ProgressBar.svelte';

	interface Props {
		model: AiModel;
		/** 保存メモの提案に使っているモデルか */
		selected: boolean;
		/** ダウンロード中の受信量（ダウンロード中のときだけ渡す） */
		progress?: { received: number; total: number } | null;
		/** 直前のダウンロードが失敗したか */
		failed?: boolean;
		onaction: (action: ModelAction) => void;
		class?: string;
	}

	let {
		model,
		selected,
		progress = null,
		failed = false,
		onaction,
		class: className = ''
	}: Props = $props();

	const buttons = $derived(modelButtons(model, selected));
	/** 推奨のバッジ（この PC のおすすめを優先） */
	const recommendation = $derived(recommendationKey(model));
	/** 段階 1 だけ、内容の理解が限定的であることの説明を出す */
	const stageNote = $derived(stageNoteKey(model.stage));
	const unavailable = $derived(
		model.unavailableReason ? unavailableMessage(model.unavailableReason) : null
	);
	const percent = $derived(progress ? progressPercent(progress.received, progress.total) : 0);
	const statusText = $derived(
		model.status === 'ready'
			? t('ai.status_ready')
			: model.status === 'partial'
				? t('ai.status_partial', {
						received: formatBytes(model.partialBytes),
						total: formatBytes(model.sizeBytes)
					})
				: t('ai.status_missing')
	);
</script>

<article
	class={`flex flex-direction:column gap:2 p:3 r:md b:1px|solid|border bg:bg-raised ${className}`}
	aria-label={model.displayName}
>
	<div class="flex align-items:center justify-content:space-between gap:3">
		<div class="flex align-items:center gap:2 min-w:0">
			<span class="type-body font-weight:600">{model.displayName}</span>
			{#if recommendation}
				<Badge variant="sync" label={t(recommendation)} />
			{/if}
		</div>
		{#if selected}
			<Badge variant="saved" label={t('ai.status_selected')} />
		{/if}
	</div>

	<p class="m:0 type-small fg:fg-muted">
		{t('ai.stage', { stage: model.stage, level: t(stageLevelKey(model.stage)) })} ・ {t('ai.size', {
			size: formatBytes(model.sizeBytes)
		})} ・ {t('ai.memory_need', { memory: formatBytes(model.minMemoryBytes) })}
	</p>
	{#if stageNote}
		<p class="m:0 type-small fg:fg-muted">{t(stageNote)}</p>
	{/if}
	<p class="m:0 type-small fg:fg-muted">{t('ai.license', { name: model.licenseName })}</p>

	{#if progress}
		<ProgressBar value={percent} label={t('ai.progress_label', { name: model.displayName })} />
	{:else}
		<p class="m:0 type-small">{statusText}</p>
	{/if}

	{#if unavailable}
		<p class="m:0 type-small fg:fg-muted">{t(unavailable.key, unavailable.params)}</p>
	{/if}
	{#if failed}
		<p class="m:0 type-small fg:state-danger">{t('ai.download_failed')}</p>
	{/if}

	<div class="flex justify-content:flex-end gap:2 flex-flow:row|wrap">
		{#if buttons.cancel}
			<Button variant="secondary" size="sm" onclick={() => onaction('cancel')}>
				{t('ai.cancel')}
			</Button>
		{/if}
		{#if buttons.download}
			<Button size="sm" onclick={() => onaction('download')}>{t('ai.download')}</Button>
		{/if}
		{#if buttons.resume}
			<Button size="sm" onclick={() => onaction('download')}>{t('ai.resume')}</Button>
		{/if}
		{#if buttons.use}
			<Button variant="secondary" size="sm" onclick={() => onaction('use')}>
				{t('ai.use')}
			</Button>
		{/if}
		{#if buttons.remove}
			<Button variant="ghost" size="sm" onclick={() => onaction('remove')}>
				{t('ai.remove')}
			</Button>
		{/if}
	</div>
</article>
