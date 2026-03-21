import { useState } from 'react';
import { AlertTriangle } from 'lucide-react';

interface ConfirmDialogProps {
  isOpen: boolean;
  title: string;
  message: string;
  confirmText?: string;
  requireTyping?: string;
  onConfirm: () => void;
  onCancel: () => void;
  danger?: boolean;
}

export default function ConfirmDialog({
  isOpen,
  title,
  message,
  confirmText = 'Confirm',
  requireTyping,
  onConfirm,
  onCancel,
  danger = true,
}: ConfirmDialogProps) {
  const [typed, setTyped] = useState('');

  if (!isOpen) return null;

  const canConfirm = requireTyping ? typed === requireTyping : true;

  const handleConfirm = () => {
    if (canConfirm) {
      setTyped('');
      onConfirm();
    }
  };

  const handleCancel = () => {
    setTyped('');
    onCancel();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/50" onClick={handleCancel} />
      <div className="relative bg-white dark:bg-gray-800 rounded-xl shadow-xl p-6 w-full max-w-md mx-4 border border-gray-200 dark:border-gray-700">
        <div className="flex items-start gap-4 mb-4">
          {danger && (
            <div className="flex-shrink-0 w-10 h-10 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center">
              <AlertTriangle size={20} className="text-red-600 dark:text-red-400" />
            </div>
          )}
          <div>
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">{title}</h3>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">{message}</p>
          </div>
        </div>

        {requireTyping && (
          <div className="mb-4">
            <p className="text-sm text-gray-600 dark:text-gray-400 mb-2">
              Type <span className="font-mono font-semibold text-gray-900 dark:text-gray-100">{requireTyping}</span> to confirm:
            </p>
            <input
              type="text"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-red-500 font-mono text-sm"
              autoFocus
            />
          </div>
        )}

        <div className="flex gap-3 justify-end">
          <button
            onClick={handleCancel}
            className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-600 transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleConfirm}
            disabled={!canConfirm}
            className={`px-4 py-2 text-sm font-medium text-white rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
              danger
                ? 'bg-red-600 hover:bg-red-700 disabled:bg-red-400'
                : 'bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400'
            }`}
          >
            {confirmText}
          </button>
        </div>
      </div>
    </div>
  );
}
