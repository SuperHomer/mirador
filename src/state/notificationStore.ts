import { create } from "zustand";
import { NotificationDto } from "../bindings";

interface NotificationStore {
  notifications: NotificationDto[];
  setAll: (list: NotificationDto[]) => void;
  append: (n: NotificationDto) => void;
}

const CAP = 200;

export const useNotificationStore = create<NotificationStore>((set) => ({
  notifications: [],
  setAll: (notifications) => set({ notifications }),
  append: (n) =>
    set((s) => ({
      notifications: [...s.notifications, n].slice(-CAP),
    })),
}));
