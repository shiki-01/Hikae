<script lang="ts">
	// 表示の本体（iframe の中）。内容の種類ごとに、既存の表示コンポーネントで描く。
	// ホストとは props（データ）でだけつながり、取得（tauri・fetch）は行わない。
	import { t, type MessageKey } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import SheetGrid from './SheetGrid.svelte';
	import { plainSheetView } from '#lib/features/compare/xlsx-grid.js';
	import { isTruncatedPreview, unavailableMessageKey } from '#lib/features/compare/preview-view.js';
	import type { PreviewUnavailableReason } from '#lib/api/types.js';
	import type { RenderContent, RenderFile } from '#lib/features/extensions/renderer-protocol.js';

	interface Props {
		file: RenderFile;
		content: RenderContent | undefined;
		/** 表示できなかったときに、ホストへ知らせる */
		onfail: (code: 'render-failed' | 'unsupported') => void;
		/** 「ファイルを開く」の要求 */
		onopen: () => void;
	}

	let { file, content, onfail, onopen }: Props = $props();

	const centered =
		'flex flex-direction:column gap:3 align-items:center justify-content:center h:100% p:6';

	/** ホストから届く理由のうち、文言のあるもの（それ以外は汎用の文言） */
	const KNOWN_REASONS: readonly string[] = ['too_large', 'unsupported_format', 'corrupt', 'binary'];

	const reasonKey = $derived.by((): MessageKey => {
		const reason = file.reason;
		return reason !== undefined && KNOWN_REASONS.includes(reason)
			? unavailableMessageKey(reason as PreviewUnavailableReason)
			: 'preview.failed';
	});

	const sheets = $derived(
		content?.kind === 'cells' ? content.sheets.map((sheet) => plainSheetView(sheet)) : []
	);

	// 画像は blob URL で描く（描き終えたら解放する）
	let imageUrl = $state<string | null>(null);
	let imageFailed = $state(false);
	$effect(() => {
		const current = content;
		if (current?.kind !== 'bytes' || current.mime === 'application/pdf') return;
		const url = URL.createObjectURL(new Blob([current.data], { type: current.mime }));
		imageUrl = url;
		imageFailed = false;
		return () => URL.revokeObjectURL(url);
	});

	// 内容が無いのに「ある」と届いたとき、表示できない種類のとき
	$effect(() => {
		if (file.state === 'ok' && content === undefined) onfail('render-failed');
	});
</script>

<svelte:boundary onerror={() => onfail('render-failed')}>
	<div
		class="flex flex-direction:column flex:1 min-h:0 min-w:0 h:100% bg:bg"
		aria-label={file.name}
	>
		{#if file.state === 'missing'}
			<div class={centered}>
				<p class="m:0 type-body fg:fg-muted text-align:center">{t('preview.not_found')}</p>
			</div>
		{:else if file.state === 'unavailable'}
			<div class={centered}>
				<p class="m:0 type-body text-align:center">{t(reasonKey)}</p>
				<p class="m:0 type-small fg:fg-muted text-align:center">{t('preview.unavailable.safe')}</p>
				<Button variant="secondary" onclick={onopen}>{t('preview.extension.open')}</Button>
			</div>
		{:else if content?.kind === 'bytes'}
			{#if content.mime === 'application/pdf'}
				<!-- pdf.js は重いため、pdf を開いたときだけ読み込む -->
				{#await import('#lib/features/compare/PdfViewer.svelte') then module}
					{@const PdfComponent = module.default}
					<PdfComponent data={content.data} label={file.name} {onopen} />
				{:catch}
					<div class={centered}>
						<p class="m:0 type-body fg:fg-muted text-align:center">{t('preview.pdf.failed')}</p>
						<Button variant="secondary" onclick={onopen}>{t('preview.extension.open')}</Button>
					</div>
				{/await}
			{:else}
				<div
					class="flex align-items:center justify-content:center flex:1 min-h:0 p:3 overflow:auto bg:bg-subtle"
				>
					{#if imageFailed}
						<div class={centered}>
							<p class="m:0 type-body fg:fg-muted text-align:center">{t('preview.image.failed')}</p>
							<Button variant="secondary" onclick={onopen}>{t('preview.extension.open')}</Button>
						</div>
					{:else if imageUrl === null}
						<Spinner />
					{:else}
						<img
							src={imageUrl}
							alt={file.name}
							class="max-w:100% max-h:100% object-fit:contain"
							onerror={() => (imageFailed = true)}
						/>
					{/if}
				</div>
			{/if}
		{:else if content?.kind === 'cells'}
			{#if isTruncatedPreview({ kind: 'xlsx', sheets: content.sheets })}
				<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
					{t('preview.truncated')}
				</p>
			{/if}
			<SheetGrid {sheets} ariaLabel={file.name} />
		{:else if content}
			{#if content.truncated}
				<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
					{t('preview.truncated')}
				</p>
			{/if}
			<div
				class="flex flex-direction:column gap:2 flex:1 min-h:0 overflow:auto p:4"
				aria-label={file.name}
			>
				{#if content.kind === 'text'}
					{#if content.text === ''}
						<p class="m:0 type-body fg:fg-muted">{t('preview.empty')}</p>
					{:else}
						<pre
							class="m:0 type-mono white-space:pre-wrap overflow-wrap:anywhere">{content.text}</pre>
					{/if}
				{:else if content.kind === 'paragraphs'}
					{#if content.paragraphs.length === 0}
						<p class="m:0 type-body fg:fg-muted">{t('preview.empty')}</p>
					{/if}
					{#each content.paragraphs as paragraph, index (index)}
						<!-- 空の段落は高さを保って余白になる -->
						<p class="m:0 min-h:22px type-body white-space:pre-wrap overflow-wrap:anywhere">
							{paragraph}
						</p>
					{/each}
				{:else if content.kind === 'pages'}
					{#each content.pages as slide (slide.index)}
						<section
							class="flex flex-direction:column gap:2 p:4 r:md br:1px|solid|border bg:bg-raised"
						>
							<h3 class="m:0 type-small fg:fg-muted">{t('preview.slide', { n: slide.index })}</h3>
							{#if slide.paragraphs.length === 0}
								<p class="m:0 type-small fg:fg-muted">{t('preview.slide_empty')}</p>
							{/if}
							{#each slide.paragraphs as paragraph, index (index)}
								<p class="m:0 type-body white-space:pre-wrap overflow-wrap:anywhere">{paragraph}</p>
							{/each}
						</section>
					{/each}
				{/if}
			</div>
		{:else}
			<div class={centered}>
				<p class="m:0 type-body fg:fg-muted text-align:center">{t('preview.failed')}</p>
			</div>
		{/if}
	</div>
</svelte:boundary>
