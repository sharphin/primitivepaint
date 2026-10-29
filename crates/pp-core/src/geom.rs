/// 半開区間 [x0, x1) × [y0, y1) の整数矩形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Rect { x0, y0, x1, y1 }
    }

    pub fn from_size(width: u32, height: u32) -> Self {
        Rect::new(0, 0, width as i32, height as i32)
    }

    pub fn is_empty(&self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    pub fn width(&self) -> u32 {
        (self.x1 - self.x0).max(0) as u32
    }

    pub fn height(&self) -> u32 {
        (self.y1 - self.y0).max(0) as u32
    }

    /// 点 (x, y) が矩形の中にあるか。
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x0 as f32 && x < self.x1 as f32 && y >= self.y0 as f32 && y < self.y1 as f32
    }

    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        Rect::new(self.x0.min(other.x0), self.y0.min(other.y0), self.x1.max(other.x1), self.y1.max(other.y1))
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let r = Rect::new(self.x0.max(other.x0), self.y0.max(other.y0), self.x1.min(other.x1), self.y1.min(other.y1));
        if r.is_empty() { Rect::EMPTY } else { r }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_ignores_empty() {
        let a = Rect::new(1, 2, 3, 4);
        assert_eq!(Rect::EMPTY.union(&a), a);
        assert_eq!(a.union(&Rect::EMPTY), a);
    }

    #[test]
    fn intersect_disjoint_is_empty() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(20, 20, 30, 30);
        assert!(a.intersect(&b).is_empty());
        assert_eq!(a.intersect(&Rect::new(5, 5, 15, 15)), Rect::new(5, 5, 10, 10));
    }
}
