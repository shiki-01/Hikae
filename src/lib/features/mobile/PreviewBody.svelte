<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import { base64ToBytes } from './base64';
	import type { MobilePreview } from '#lib/api/mobile-types.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';

	interface Props {
		preview: MobilePreview;
		name: string;
		ondownload: () => void;
	}

	let { preview, name, ondownload }: Props = $props();

	/** PDF のバイト列（pdf.js に渡す）。PDF のときだけ作る */
	const pdfData = $derived(
		preview.kind === 'pdf' ? (base64ToBytes(preview.contentBase64).buffer as ArrayBuffer) : null
	);
</script>

{#if preview.kind === 'text'}
	<div class="flex flex-direction:column gap:2 p:4">
		{#if preview.truncated}
			<p class="m:0 type-small fg:fg-muted">{t('mobile.preview.text_truncated')}</p>
		{/if}
		<pre class="m:0 type-mono white-space:pre-wrap word-break:break-all fg:fg">{preview.text}</pre>
	</div>
{:else if preview.kind === 'image'}
	<div class="p:4">
		<img
			src={`data:${preview.mime};base64,${preview.contentBase64}`}
			alt={t('mobile.preview.image_alt', { name })}
			class="w:100% h:auto display:block"
		/>
	</div>
{:else if preview.kind === 'pdf' && pdfData}
	<div class="flex flex-direction:column flex:1 min-h:0">
		{#await import('#lib/features/compare/PdfViewer.svelte')}
			<div class="flex justify-content:center p:6"><Spinner /></div>
		{:then viewer}
			<viewer.default data={pdfData} label={name} />
		{:catch}
			<p class="m:0 p:4 type-body">{t('preview.pdf.failed')}</p>
		{/await}
	</div>
{:else if preview.kind === 'document'}
	<div class="flex flex-direction:column">
		{#if preview.sections.length === 0}
			<p class="m:0 p:4 type-body fg:fg-muted">{t('mobile.preview.empty_document')}</p>
		{/if}
		{#each preview.sections as section, index (index)}
			<section class="flex flex-direction:column gap:2 p:4 bb:1px|solid|border">
				{#if section.title}
					<h2 class="m:0 type-heading">{section.title}</h2>
				{/if}
				{#each section.paragraphs as paragraph, paragraphIndex (paragraphIndex)}
					<p class="m:0 type-body white-space:pre-wrap">{paragraph}</p>
				{/each}
				{#if section.rows}
					<div class="overflow-x:auto">
						<table class="border-collapse:collapse type-small">
							<tbody>
								{#each section.rows as row, rowIndex (rowIndex)}
									<tr>
										{#each row as cell, cellIndex (cellIndex)}
											<td class="b:1px|solid|border p:2 white-space:pre-wrap">{cell}</td>
										{/each}
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</section>
		{/each}
	</div>
{:else if preview.kind === 'unsupported'}
	<div class="flex flex-direction:column gap:3 p:6">
		<p class="m:0 type-heading">{t('mobile.preview.unsupported.title')}</p>
		<p class="m:0 type-body fg:fg-muted">{t('mobile.preview.unsupported.message')}</p>
		<div>
			<Button class="min-h:44px" onclick={ondownload}>{t('mobile.common.download')}</Button>
		</div>
	</div>
{:else if preview.kind === 'tooLarge'}
	<div class="flex flex-direction:column gap:3 p:6">
		<p class="m:0 type-heading">{t('mobile.preview.too_large.title', { name })}</p>
		<p class="m:0 type-body fg:fg-muted">{t('mobile.preview.too_large.message')}</p>
	</div>
{/if}
