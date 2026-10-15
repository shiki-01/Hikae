<script lang="ts">
	import { formatDateTime } from '#lib/i18n/format.js';
	import { t } from '#lib/i18n/index.js';
	import Spinner from '#lib/components/Spinner.svelte';
	import type { MobileRepo } from '#lib/api/mobile-types.js';
	import ErrorNotice from './ErrorNotice.svelte';
	import ScreenHeader from './ScreenHeader.svelte';
	import { chooseRepo, endRepoChange, nav } from './nav.svelte';
	import { useRepos } from './queries';

	const repos = useRepos();

	function pushedLabel(repo: MobileRepo): string | null {
		if (!repo.pushedAt) return null;
		const date = new Date(repo.pushedAt);
		return Number.isNaN(date.getTime())
			? null
			: t('mobile.repos.pushed', { when: formatDateTime(date) });
	}
</script>

<ScreenHeader
	title={t('mobile.repos.title')}
	onback={nav.repo !== null ? endRepoChange : undefined}
/>

<div class="flex flex-direction:column flex:1 min-h:0 overflow:auto">
	<p class="m:0 p:4 type-body fg:fg-muted">{t('mobile.repos.description')}</p>
	{#if repos.isPending}
		<div class="flex justify-content:center p:6"><Spinner /></div>
	{:else if repos.isError}
		<ErrorNotice
			error={repos.error}
			onretry={() => void repos.refetch()}
			onlogin={() => (nav.needsLogin = true)}
		/>
	{:else if repos.data}
		{#if repos.data.length === 0}
			<p class="m:0 p:4 type-body fg:fg-muted">{t('mobile.repos.empty')}</p>
		{:else}
			<ul class="list-style:none m:0 p:0">
				{#each repos.data as repo (`${repo.owner}/${repo.name}`)}
					{@const pushed = pushedLabel(repo)}
					<li class="bb:1px|solid|border">
						<button
							type="button"
							onclick={() => chooseRepo(repo)}
							class="flex flex-direction:column gap:1 w:100% min-h:44px px:4 py:3 b:0 bg:transparent fg:fg text-align:left cursor:pointer bg:bg-subtle:hover"
						>
							<span class="type-heading overflow:hidden text-overflow:ellipsis white-space:nowrap"
								>{repo.name}</span
							>
							<span class="type-small fg:fg-muted">
								{repo.owner} ・ {repo.private
									? t('mobile.repos.private')
									: t('mobile.repos.public')}
								{#if pushed}
									・ {pushed}
								{/if}
							</span>
							{#if repo.description}
								<span
									class="type-small fg:fg-muted overflow:hidden text-overflow:ellipsis white-space:nowrap"
									>{repo.description}</span
								>
							{/if}
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	{/if}
</div>
