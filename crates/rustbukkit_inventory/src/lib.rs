use std::num::NonZeroU8;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    GrassBlock,
    Dirt,
    Stone,
    Cobblestone,
    Sand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ItemStack {
    kind:  ItemKind,
    count: NonZeroU8,
}

impl<'de> Deserialize<'de> for ItemStack {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct SavedStack {
            kind:  ItemKind,
            count: u8,
        }

        let saved = SavedStack::deserialize(deserializer)?;
        Self::new(saved.kind, saved.count)
            .ok_or_else(|| serde::de::Error::custom("item count must be between 1 and 64"))
    }
}

/// A mutable player-inventory slot (main inventory, hotbar, or offhand).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InventorySlot(u8);

impl InventorySlot {
    pub fn new(index: u8) -> Option<Self> {
        (9..=45).contains(&index).then_some(Self(index))
    }

    pub fn index(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryAction {
    Rearrange,
    DropOne(u8),
    DropStack(u8),
    DropCursorOne,
    DropCursorStack,
}

impl ItemStack {
    pub fn new(kind: ItemKind, count: u8) -> Option<Self> {
        (count <= 64)
            .then(|| NonZeroU8::new(count).map(|count| Self { kind, count }))
            .flatten()
    }

    pub fn kind(self) -> ItemKind {
        self.kind
    }

    pub fn count(self) -> u8 {
        self.count.get()
    }

    fn take_one(self) -> Option<Self> {
        Self::new(self.kind, self.count.get() - 1)
    }

    fn with_count(self, count: u8) -> Option<Self> {
        Self::new(self.kind, count)
    }
}

#[derive(Debug, Clone)]
pub struct Inventory {
    slots:    [Option<ItemStack>; 46],
    carried:  Option<ItemStack>,
    selected: u8,
}

impl Serialize for Inventory {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut saved = serializer.serialize_struct("Inventory", 3)?;
        saved.serialize_field("slots", &self.slots.as_slice())?;
        saved.serialize_field("carried", &self.carried)?;
        saved.serialize_field("selected", &self.selected)?;
        saved.end()
    }
}

impl<'de> Deserialize<'de> for Inventory {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct SavedInventory {
            slots:    Vec<Option<ItemStack>>,
            carried:  Option<ItemStack>,
            selected: u8,
        }

        let saved = SavedInventory::deserialize(deserializer)?;
        let slots: [Option<ItemStack>; 46] = saved
            .slots
            .try_into()
            .map_err(|_: Vec<Option<ItemStack>>| serde::de::Error::custom("inventory must have 46 slots"))?;
        if saved.selected >= 9 || slots[..9].iter().any(Option::is_some) {
            return Err(serde::de::Error::custom("invalid player inventory slots or selection"));
        }
        Ok(Self {
            slots,
            carried: saved.carried,
            selected: saved.selected,
        })
    }
}

impl Inventory {
    pub fn empty(selected: u8) -> Option<Self> {
        (selected < 9).then_some(Self {
            slots: [None; 46],
            carried: None,
            selected,
        })
    }

    pub fn test_inventory(selected: u8) -> Option<Self> {
        Self::empty(selected).map(|mut inventory| {
            for (slot, kind) in [
                ItemKind::GrassBlock,
                ItemKind::Dirt,
                ItemKind::Stone,
                ItemKind::Cobblestone,
            ]
            .into_iter()
            .enumerate()
            {
                inventory.slots[36 + slot] = ItemStack::new(kind, 64);
            }
            inventory
        })
    }

    pub fn get(&self, slot: InventorySlot) -> Option<ItemStack> {
        self.slots[usize::from(slot.index())]
    }

    pub fn set(&mut self, slot: InventorySlot, stack: Option<ItemStack>) -> Option<ItemStack> {
        std::mem::replace(&mut self.slots[usize::from(slot.index())], stack)
    }

    /// Insert into matching stacks, then empty main-inventory and hotbar slots.
    /// Returns the part that did not fit.
    pub fn insert(&mut self, stack: ItemStack) -> Option<ItemStack> {
        let mut remaining = stack.count();
        for current in self.slots[9..45].iter_mut().flatten() {
            if current.kind() == stack.kind() && current.count() < 64 {
                let moved = remaining.min(64 - current.count());
                *current = current
                    .with_count(current.count() + moved)
                    .expect("merged stack fits");
                remaining -= moved;
                if remaining == 0 {
                    return None;
                }
            }
        }
        for slot in &mut self.slots[9..45] {
            if slot.is_none() {
                let moved = remaining.min(64);
                *slot = stack.with_count(moved);
                remaining -= moved;
                if remaining == 0 {
                    return None;
                }
            }
        }
        stack.with_count(remaining)
    }

    /// Remove up to `count` items. Zero is not a valid request.
    pub fn take(&mut self, slot: InventorySlot, count: NonZeroU8) -> Option<ItemStack> {
        let current = self.get(slot)?;
        let removed = current.count().min(count.get());
        self.set(slot, current.with_count(current.count() - removed));
        current.with_count(removed)
    }

    pub fn take_stack(&mut self, slot: InventorySlot) -> Option<ItemStack> {
        self.set(slot, None)
    }

    pub fn take_carried(&mut self) -> Option<ItemStack> {
        self.carried.take()
    }

    /// Remove and return every stack, for mechanics such as death drops.
    pub fn take_all(&mut self) -> Vec<ItemStack> {
        let mut stacks = Vec::new();
        for slot in &mut self.slots[9..46] {
            if let Some(stack) = slot.take() {
                stacks.push(stack);
            }
        }
        if let Some(stack) = self.carried.take() {
            stacks.push(stack);
        }
        stacks
    }

    pub fn take_selected(&mut self, entire_stack: bool) -> Option<ItemStack> {
        let slot = InventorySlot(36 + self.selected);
        if entire_stack {
            self.take_stack(slot)
        } else {
            self.take(slot, NonZeroU8::MIN)
        }
    }

    pub fn selected(&self) -> u8 {
        self.selected
    }

    pub fn select(&mut self, slot: u8) -> bool {
        if slot >= 9 {
            return false;
        }
        self.selected = slot;
        true
    }

    pub fn window_slot(&self, slot: u8) -> Option<ItemStack> {
        self.slots.get(usize::from(slot)).copied().flatten()
    }

    pub fn carried(&self) -> Option<ItemStack> {
        self.carried
    }

    pub fn selected_stack(&self) -> Option<ItemStack> {
        self.slots[36 + usize::from(self.selected)]
    }

    pub fn consume_selected(&mut self) -> Option<ItemStack> {
        self.take_selected(false)?;
        self.selected_stack()
    }

    pub fn drop_selected(&mut self, entire_stack: bool) -> bool {
        self.take_selected(entire_stack).is_some()
    }

    pub fn apply_client_changes(
        &mut self,
        action: InventoryAction,
        changes: &[(u8, Option<ItemStack>)],
        carried: Option<ItemStack>,
    ) -> bool {
        let mut next = self.clone();
        let mut seen = [false; 46];
        for &(slot, stack) in changes {
            let index = usize::from(slot);
            if !(9..46).contains(&index) || seen[index] {
                return false;
            }
            seen[index] = true;
            next.slots[index] = stack;
        }
        next.carried = carried;
        let valid = match action {
            InventoryAction::Rearrange => self.totals() == next.totals(),
            InventoryAction::DropOne(slot) | InventoryAction::DropStack(slot) => {
                let index = usize::from(slot);
                let Some(current) = self.slots.get(index).copied().flatten() else {
                    return false;
                };
                let mut expected = self.clone();
                expected.slots[index] = match action {
                    InventoryAction::DropOne(_) => current.take_one(),
                    InventoryAction::DropStack(_) => None,
                    _ => unreachable!(),
                };
                next.slots == expected.slots && next.carried == expected.carried
            }
            InventoryAction::DropCursorOne | InventoryAction::DropCursorStack => {
                let Some(current) = self.carried else {
                    return false;
                };
                let mut expected = self.clone();
                expected.carried = match action {
                    InventoryAction::DropCursorOne => current.take_one(),
                    InventoryAction::DropCursorStack => None,
                    _ => unreachable!(),
                };
                next.slots == expected.slots && next.carried == expected.carried
            }
        };
        if !valid {
            return false;
        }
        *self = next;
        true
    }

    fn totals(&self) -> [u16; 5] {
        let mut totals = [0; 5];
        for stack in self.slots.iter().copied().chain([self.carried]).flatten() {
            let index = match stack.kind() {
                ItemKind::GrassBlock => 0,
                ItemKind::Dirt => 1,
                ItemKind::Stone => 2,
                ItemKind::Cobblestone => 3,
                ItemKind::Sand => 4,
            };
            totals[index] += u16::from(stack.count());
        }
        totals
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU8;

    use super::{Inventory, InventoryAction, InventorySlot, ItemKind, ItemStack};

    fn slot(index: u8) -> InventorySlot {
        InventorySlot::new(index).unwrap()
    }

    #[test]
    fn inserts_into_empty_slot_and_merges_partial_stack() {
        let mut inventory = Inventory::empty(0).unwrap();
        let dirt = ItemStack::new(ItemKind::Dirt, 20).unwrap();
        assert_eq!(inventory.insert(dirt), None);
        assert_eq!(inventory.get(slot(9)), Some(dirt));
        assert_eq!(inventory.insert(ItemStack::new(ItemKind::Dirt, 50).unwrap()), None);
        assert_eq!(inventory.get(slot(9)), ItemStack::new(ItemKind::Dirt, 64));
        assert_eq!(inventory.get(slot(10)), ItemStack::new(ItemKind::Dirt, 6));
    }

    #[test]
    fn full_inventory_returns_remainder_without_deleting_items() {
        let mut inventory = Inventory::empty(0).unwrap();
        for index in 9..45 {
            inventory.set(slot(index), ItemStack::new(ItemKind::Stone, 64));
        }
        inventory.set(slot(45), ItemStack::new(ItemKind::Dirt, 1));
        let dirt = ItemStack::new(ItemKind::Dirt, 12).unwrap();
        assert_eq!(inventory.insert(dirt), Some(dirt));
        assert_eq!(inventory.get(slot(9)), ItemStack::new(ItemKind::Stone, 64));
        assert_eq!(inventory.get(slot(45)), ItemStack::new(ItemKind::Dirt, 1));
    }

    #[test]
    fn partially_filled_inventory_returns_exact_remainder() {
        let mut inventory = Inventory::empty(0).unwrap();
        for index in 9..45 {
            inventory.set(slot(index), ItemStack::new(ItemKind::Dirt, 64));
        }
        inventory.set(slot(12), ItemStack::new(ItemKind::Dirt, 59));
        assert_eq!(
            inventory.insert(ItemStack::new(ItemKind::Dirt, 10).unwrap()),
            ItemStack::new(ItemKind::Dirt, 5)
        );
        assert_eq!(inventory.get(slot(12)), ItemStack::new(ItemKind::Dirt, 64));
    }

    #[test]
    fn take_one_and_full_stack_return_removed_items() {
        let mut inventory = Inventory::empty(0).unwrap();
        inventory.set(slot(36), ItemStack::new(ItemKind::GrassBlock, 3));
        assert_eq!(inventory.take_selected(false), ItemStack::new(ItemKind::GrassBlock, 1));
        assert_eq!(inventory.get(slot(36)), ItemStack::new(ItemKind::GrassBlock, 2));
        assert_eq!(
            inventory.take(slot(36), NonZeroU8::new(5).unwrap()),
            ItemStack::new(ItemKind::GrassBlock, 2)
        );
        assert_eq!(inventory.get(slot(36)), None);
        inventory.set(slot(36), ItemStack::new(ItemKind::Dirt, 7));
        assert_eq!(inventory.take_selected(true), ItemStack::new(ItemKind::Dirt, 7));
        assert_eq!(inventory.take_stack(slot(36)), None);
    }

    #[test]
    fn rejects_invalid_slots_and_saved_invariants() {
        assert_eq!(InventorySlot::new(8), None);
        assert_eq!(InventorySlot::new(46), None);
        assert_eq!(ItemStack::new(ItemKind::Dirt, 0), None);
        assert_eq!(ItemStack::new(ItemKind::Dirt, 65), None);
        assert!(serde_json::from_str::<ItemStack>(r#"{"kind":"Dirt","count":65}"#).is_err());
        let inventory = Inventory::empty(0).unwrap();
        let mut saved = serde_json::to_value(&inventory).unwrap();
        saved["selected"] = 9.into();
        assert!(serde_json::from_value::<Inventory>(saved).is_err());
        assert_eq!(
            serde_json::from_value::<Inventory>(serde_json::to_value(&inventory).unwrap())
                .unwrap()
                .selected(),
            0
        );
    }

    #[test]
    fn selected_slot_consumes_to_empty() {
        let mut inventory = Inventory::test_inventory(1).unwrap();
        assert_eq!(inventory.selected_stack().unwrap().kind(), ItemKind::Dirt);
        for _ in 0..64 {
            inventory.consume_selected();
        }
        assert!(inventory.selected_stack().is_none());
        assert!(!inventory.select(9));
    }

    #[test]
    fn moving_stack_into_main_inventory_updates_selected_slot_without_minting_items() {
        let mut inventory = Inventory::test_inventory(0).unwrap();
        let grass = ItemStack::new(ItemKind::GrassBlock, 64);
        assert!(inventory.apply_client_changes(InventoryAction::Rearrange, &[(36, None), (9, grass)], None));
        assert_eq!(inventory.selected_stack(), None);
        assert_eq!(inventory.window_slot(9), grass);
        assert!(!inventory.apply_client_changes(InventoryAction::Rearrange, &[(36, grass)], None));
        assert_eq!(inventory.selected_stack(), None);
    }

    #[test]
    fn dropping_an_inventory_stack_does_not_restore_an_old_hotbar_stack() {
        let mut inventory = Inventory::test_inventory(0).unwrap();
        let grass = ItemStack::new(ItemKind::GrassBlock, 64);
        assert!(inventory.apply_client_changes(InventoryAction::Rearrange, &[(36, None), (9, grass)], None));
        assert!(inventory.apply_client_changes(InventoryAction::DropStack(9), &[(9, None)], None));
        assert_eq!(inventory.window_slot(9), None);
        assert_eq!(inventory.selected_stack(), None);
        assert!(!inventory.apply_client_changes(InventoryAction::Rearrange, &[(36, grass)], None));
    }
}
