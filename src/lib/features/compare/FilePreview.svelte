<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import SheetGrid from './SheetGrid.svelte';
	import { blobUrl, isWindowsPlatform, revForPoint } from './image-source';
	import { previewKindOf } from './preview-kind';
	import { isTruncatedPreview, unavailableMessageKey } from './preview-view';
	import { useFilePreview } from './queries';
	import { plainSheetView } from './xlsx-grid';

	interface Props {
		projectId: string;
		/** 時点 ID（current・this・cloud・短縮 ID）。「いま」は current */
		rev: string;
		path: string;
		/** 時点の表示名（支援技術向けの領域の名前） */
		label: string;
		/** 無いときの文言。並べて比べる列では「この時点にはありません」にする */
		missingText?: string;
		/** 既定のアプリで開く（表示できないときの案内） */
		onopen?: () => void;
	}

	// 時点やファイルが変わるときは、呼び出し側が本コンポーネントを作り直す（`{#key}`。画像の読み込み失敗の状態を持つため）
	let { projectId, rev, path, label, missingText, onopen }: Props = $props();

	const kind = $derived(previewKindOf(path));
	const query = useFilePreview(
		() => projectId,
		() => rev,
		() => path
	);
	const data = $derived(query.data ?? null);
	const windows = isWindowsPlatform();
	const mediaUrl = $derived(blobUrl(projectId, revForPoint(rev), path, windows));
	// 画像・pdf は、内容ではなく app-blob の URL で読む。存在の確認は内容の取得（binary）で行う
	const media = $derived(
		(kind === 'image' || kind === 'pdf') && data?.kind === 'unavailable' && data.reason === 'binary'
	);
	const sheets = $derived(
		data?.kind === 'xlsx' ? data.sheets.map((sheet) => plainSheetView(sheet)) : []
	);

	// 画像の読み込み状態（読み込み中は描かず、スピナーを出す）
	let imageState = $state<'loading' | 'loaded' | 'failed'>('loading');

	// 300ms 以上かかるときだけスピナーを出す（ちらつき防止）
	let spinnerVisible = $state(false);
	$effect(() => {
		if (!query.isPending) {
			spinnerVisible = false;
			return;
		}
		const timer = setTimeout(() => (spinnerVisible = true), 300);
		return () => clearTimeout(timer);
	});

	const centered =
		'flex flex-direction:column gap:3 align-items:center justify-content:center h:100% p:6';
</script>

<div class="flex flex-direction:column flex:1 min-h:0 min-w:0 bg:bg" data-testid="file-preview">
	{#if query.isPending}
		{#if spinnerVisible}
			<div class="flex align-items:center justify-content:center h:100%">
				<Spinner />
			</div>
		{/if}
	{:else if data === null || query.isError}
		<div class={centered}>
			<p class="m:0 type-body fg:fg-muted text-align:center">{t('preview.failed')}</p>
		</div>
	{:else if data.kind === 'not_found'}
		<div class={centered}>
			<p class="m:0 type-body fg:fg-muted text-align:center">
				{missingText ?? t('preview.not_found')}
			</p>
		</div>
	{:else if media}
		{#if kind === 'pdf'}
			<!-- pdf.js は重いため、pdf を開いたときだけ読み込む -->
			{#await import('./PdfViewer.svelte') then module}
				{@const PdfComponent = module.default}
				<PdfComponent url={mediaUrl} {label} {onopen} />
			{:catch}
				<div class={centered}>
					<p class="m:0 type-body fg:fg-muted text-align:center">{t('preview.pdf.failed')}</p>
				</div>
			{/await}
		{:else if imageState === 'failed'}
			<div class={centered}>
				<p class="m:0 type-body fg:fg-muted text-align:center">{t('preview.image.failed')}</p>
				{#if onopen}
					<Button variant="secondary" onclick={onopen}>{t('file_action.open_default')}</Button>
				{/if}
			</div>
		{:else}
			<div
				class="flex align-items:center justify-content:center flex:1 min-h:0 p:3 overflow:auto bg:bg-subtle"
			>
				{#if imageState === 'loading'}
					<Spinner />
				{/if}
				<img
					src={mediaUrl}
					alt={label}
					class="max-w:100% max-h:100% object-fit:contain"
					style:display={imageState === 'loading' ? 'none' : undefined}
					onload={() => (imageState = 'loaded')}
					onerror={() => (imageState = 'failed')}
				/>
			</div>
		{/if}
	{:else if data.kind === 'unavailable'}
		<div class={centered}>
			<p class="m:0 type-body text-align:center">{t(unavailableMessageKey(data.reason))}</p>
			<p class="m:0 type-small fg:fg-muted text-align:center">{t('preview.unavailable.safe')}</p>
			{#if onopen}
				<Button variant="secondary" onclick={onopen}>{t('file_action.open_default')}</Button>
			{/if}
		</div>
	{:else if data.kind === 'xlsx'}
		{#if isTruncatedPreview(data)}
			<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
				{t('preview.truncated')}
			</p>
		{/if}
		<SheetGrid {sheets} ariaLabel={label} />
	{:else}
		{#if isTruncatedPreview(data)}
			<p class="m:0 px:4 py:2 type-small fg:fg-muted bb:1px|solid|border">
				{t('preview.truncated')}
			</p>
		{/if}
		<div
			class="flex flex-direction:column gap:2 flex:1 min-h:0 overflow:auto p:4"
			aria-label={label}
		>
			{#if data.kind === 'text'}
				{#if data.content === ''}
					<p class="m:0 type-body fg:fg-muted">{t('preview.empty')}</p>
				{:else}
					<pre
						class="m:0 type-mono white-space:pre-wrap overflow-wrap:anywhere">{data.content}</pre>
				{/if}
			{:else if data.kind === 'docx'}
				{#if data.paragraphs.length === 0}
					<p class="m:0 type-body fg:fg-muted">{t('preview.empty')}</p>
				{/if}
				{#each data.paragraphs as paragraph, index (index)}
					<!-- 空の段落は高さを保って余白になる -->
					<p class="m:0 min-h:22px type-body white-space:pre-wrap overflow-wrap:anywhere">
						{paragraph}
					</p>
				{/each}
			{:else if data.kind === 'pptx'}
				{#each data.slides as slide (slide.index)}
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
	{/if}
</div>
