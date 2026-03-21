import { useQuery } from '@tanstack/react-query';
import { Server, Clock, HardDrive, Activity, Database, Layers } from 'lucide-react';
import api from '../lib/api';

interface StatusData {
  version: string;
  uptime_secs: number;
  data_dir: string;
  port: number;
  wal_lsn: number;
  buffer_pool_frames: number;
  buffer_pool_dirty: number;
  table_count: number;
}

function formatUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

interface StatCardProps {
  title: string;
  value: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  sub?: string;
}

function StatCard({ title, value, icon: Icon, sub }: StatCardProps) {
  return (
    <div className="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-5">
      <div className="flex items-start justify-between">
        <div>
          <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">{title}</p>
          <p className="text-2xl font-bold text-gray-900 dark:text-gray-100 mt-1">{value}</p>
          {sub && <p className="text-xs text-gray-400 dark:text-gray-500 mt-1">{sub}</p>}
        </div>
        <div className="w-10 h-10 rounded-lg bg-blue-50 dark:bg-blue-900/30 flex items-center justify-center">
          <Icon size={20} className="text-blue-600 dark:text-blue-400" />
        </div>
      </div>
    </div>
  );
}

export default function ServerStatusPage() {
  const { data, isLoading, error, dataUpdatedAt } = useQuery<StatusData>({
    queryKey: ['status'],
    queryFn: () => api.get('/status').then((r) => r.data),
    refetchInterval: 10000,
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-gray-500 dark:text-gray-400">Loading server status...</div>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-700 rounded-xl p-4 text-red-700 dark:text-red-400">
        Failed to load server status. Is the icedb admin server running?
      </div>
    );
  }

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleTimeString() : 'N/A';

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">Server Status</h1>
        <span className="text-xs text-gray-400 dark:text-gray-500">
          Last updated: {lastUpdated} (auto-refresh 10s)
        </span>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
        <StatCard
          title="Version"
          value={`v${data.version}`}
          icon={Server}
          sub="icedb admin-server"
        />
        <StatCard
          title="Uptime"
          value={formatUptime(data.uptime_secs)}
          icon={Clock}
        />
        <StatCard
          title="Data Directory"
          value={data.data_dir}
          icon={HardDrive}
          sub={`PostgreSQL port: ${data.port}`}
        />
        <StatCard
          title="WAL LSN"
          value={String(data.wal_lsn)}
          icon={Activity}
          sub="Current write-ahead log position"
        />
        <StatCard
          title="Buffer Pool"
          value={`${data.buffer_pool_frames} frames`}
          icon={Layers}
          sub={`${data.buffer_pool_dirty} dirty pages`}
        />
        <StatCard
          title="Tables"
          value={String(data.table_count)}
          icon={Database}
          sub="In public schema"
        />
      </div>

      <div className="mt-6 bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-5">
        <h2 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-3">Raw Status</h2>
        <pre className="text-xs font-mono text-gray-600 dark:text-gray-400 bg-gray-50 dark:bg-gray-900 rounded-lg p-4 overflow-auto">
          {JSON.stringify(data, null, 2)}
        </pre>
      </div>
    </div>
  );
}
