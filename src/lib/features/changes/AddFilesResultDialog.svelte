<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import Button from '#lib/components/Button.svelte';
	import Dialog from '#lib/components/Dialog.svelte';
	import { rejectReasonText, type AddFilesSummary } from './add-files-result';

	interface Props {
		summary: AddFilesSummary | null;
		onclose: () => void;
	}

	let { summary, onclose }: Props = $props();
</script>

<Dialog
	open={summary !== null}
	variant={summary && summary.rejected.length > 0 ? 'warning' : 'confirm'}
	title={t('add_files.title')}
	description={t('add_files.description')}
	{onclose}
>
	{#if summary}
		<div class="flex flex-direction:column gap:4">
			{#if summary.rejected.length > 0}
				<section class="flex flex-direction:column gap:2">
					<h3 class="m:0 type-body font-weight:700">{t('add_files.rejected_heading')}</h3>
					<ul class="m:0 p:0 list-style:none b:1px|solid|border r:md">
						{#each summary.rejected as file (file.name)}
							<li class="flex flex-direction:column px:3 py:2 bb:1px|solid|border">
								<span class="type-body overflow-wrap:anywhere">{file.name}</span>
								<span class="type-small fg:fg-muted">{rejectReasonText(file)}</span>
							</li>
						{/each}
					</ul>
					<p class="m:0 type-small fg:fg-muted">{t('add_files.rejected_safe')}</p>
				</section>
			{/if}

			{#if summary.replaced.length > 0}
				<section class="flex flex-direction:column gap:2">
					<h3 class="m:0 type-body font-weight:700">{t('add_files.replaced_heading')}</h3>
					<ul class="m:0 p:0 list-style:none b:1px|solid|border r:md">
						{#each summary.replaced as file (file.path)}
							<li class="px:3 py:2 bb:1px|solid|border type-body overflow-wrap:anywhere">
								{file.path}
							</li>
						{/each}
					</ul>
					<p class="m:0 type-small fg:fg-muted">{t('add_files.replaced_detail')}</p>
				</section>
			{/if}

			{#if summary.replaceRefused.length > 0}
				<section class="flex flex-direction:column gap:2">
					<h3 class="m:0 type-body font-weight:700">{t('add_files.replace_refused_heading')}</h3>
					<ul class="m:0 p:0 list-style:none b:1px|solid|border r:md">
						{#each summary.replaceRefused as file (file.path)}
							<li class="px:3 py:2 bb:1px|solid|border type-body overflow-wrap:anywhere">
								{file.path}
							</li>
						{/each}
					</ul>
					<p class="m:0 type-small fg:fg-muted">{t('add_files.replace_refused_detail')}</p>
				</section>
			{/if}

			{#if summary.renamed.length > 0}
				<section class="flex flex-direction:column gap:2">
					<h3 class="m:0 type-body font-weight:700">{t('add_files.renamed_heading')}</h3>
					<ul class="m:0 p:0 list-style:none b:1px|solid|border r:md">
						{#each summary.renamed as file (file.path)}
							<li class="px:3 py:2 bb:1px|solid|border type-body overflow-wrap:anywhere">
								{file.path}
							</li>
						{/each}
					</ul>
				</section>
			{/if}

			{#if summary.skippedLinkCount > 0}
				<section class="flex flex-direction:column gap:2">
					<h3 class="m:0 type-body font-weight:700">{t('add_files.skipped_links_heading')}</h3>
					<ul class="m:0 p:0 list-style:none b:1px|solid|border r:md">
						{#each summary.skippedLinks as item (item.name)}
							<li class="px:3 py:2 bb:1px|solid|border type-body overflow-wrap:anywhere">
								{item.name}
							</li>
						{/each}
						{#if summary.skippedLinkCount > summary.skippedLinks.length}
							<li class="px:3 py:2 type-small fg:fg-muted">
								{t('add_files.skipped_more', {
									count: summary.skippedLinkCount - summary.skippedLinks.length
								})}
							</li>
						{/if}
					</ul>
					<p class="m:0 type-small fg:fg-muted">{t('add_files.skipped_links_detail')}</p>
				</section>
			{/if}

			{#if summary.large.length > 0}
				<section class="flex flex-direction:column gap:2">
					<h3 class="m:0 type-body font-weight:700">{t('add_files.large_heading')}</h3>
					<ul class="m:0 p:0 list-style:none b:1px|solid|border r:md">
						{#each summary.large as file (file.path)}
							<li class="px:3 py:2 bb:1px|solid|border type-body overflow-wrap:anywhere">
								{file.path}
							</li>
						{/each}
					</ul>
					<p class="m:0 type-small fg:fg-muted">{t('add_files.large_detail')}</p>
				</section>
			{/if}

			{#if summary.skippedHiddenCount > 0}
				<p class="m:0 type-small fg:fg-muted">
					{t('add_files.skipped_hidden', { count: summary.skippedHiddenCount })}
				</p>
			{/if}
		</div>
	{/if}

	{#snippet actions()}
		<Button onclick={onclose}>{t('add_files.close')}</Button>
	{/snippet}
</Dialog>
