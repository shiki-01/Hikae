<script lang="ts">
	import Download from '@lucide/svelte/icons/download';
	import { t } from '#lib/i18n/index.js';
	import type { MobileVersion } from '#lib/api/mobile-types.js';
	import Button from '#lib/components/Button.svelte';
	import Spinner from '#lib/components/Spinner.svelte';
	import ErrorNotice from './ErrorNotice.svelte';
	import PreviewBody from './PreviewBody.svelte';
	import ScreenHeader from './ScreenHeader.svelte';
	import VersionsDialog from './VersionsDialog.svelte';
	import { downloadEntry } from './mobile-actions';
	import { nav, openPreview, type PreviewTarget } from './nav.svelte';
	import { usePreview } from './queries';

	interface Props {
		target: PreviewTarget;
		onback: () => void;
	}

	let { target, onback }: Props = $props();

	const repo = $derived(nav.repo ?? { owner: '', name: '' });

	const preview = usePreview(
		() => repo.owner,
		() => repo.name,
		() => target.path,
		() => target.sha
	);

	let versionsOpen = $state(false);

	function viewVersion(version: MobileVersion): void {
		versionsOpen = false;
		openPreview({
			path: target.path,
			name: target.name,
			sha: version.sha,
			versionLabel: target.versionLabel
		});
	}
</script>

{#snippet actions()}
	<Button
		variant="ghost"
		size="sm"
		class="min-h:44px"
		ariaLabel={t('mobile.common.download')}
		onclick={() => void downloadEntry(repo.owner, repo.name, target.path, target.sha)}
	>
		<Download size={16} aria-hidden="true" />
		<span>{t('mobile.common.download')}</span>
	</Button>
	<Button variant="ghost" size="sm" class="min-h:44px" onclick={() => (versionsOpen = true)}
		>{t('mobile.common.versions')}</Button
	>
{/snippet}

<ScreenHeader
	title={target.name}
	subtitle={target.versionLabel ? t('mobile.preview.version', { when: target.versionLabel }) : null}
	{onback}
	{actions}
/>

<div class="flex flex-direction:column flex:1 min-h:0 overflow:auto">
	{#if preview.isPending}
		<div class="flex justify-content:center p:6"><Spinner /></div>
	{:else if preview.isError}
		<ErrorNotice
			error={preview.error}
			onretry={() => void preview.refetch()}
			onlogin={() => (nav.needsLogin = true)}
		/>
	{:else if preview.data}
		<PreviewBody
			preview={preview.data}
			name={target.name}
			ondownload={() => void downloadEntry(repo.owner, repo.name, target.path, target.sha)}
		/>
	{/if}
</div>

{#if versionsOpen}
	<VersionsDialog
		owner={repo.owner}
		name={repo.name}
		path={target.path}
		fileName={target.name}
		onclose={() => (versionsOpen = false)}
		onview={viewVersion}
		ondownload={(version) => void downloadEntry(repo.owner, repo.name, target.path, version.sha)}
	/>
{/if}
