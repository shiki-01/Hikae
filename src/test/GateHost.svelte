<script lang="ts">
	import AppHealthGate from '#lib/features/app-health/AppHealthGate.svelte';
</script>

<!-- AppHealthGate のスモークテスト用。通常の画面の代わりに目印の文字を子として渡す -->
<AppHealthGate>
	<p>gate-child</p>
</AppHealthGate>
