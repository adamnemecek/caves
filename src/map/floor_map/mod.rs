mod tile;
mod tile_walls;
mod room;
mod tile_pos;
mod grid_size;

pub use self::tile::*;
pub use self::tile_walls::*;
pub use self::room::*;
pub use self::tile_pos::*;
pub use self::grid_size::*;

use std::fmt;
use std::ops::{Index, IndexMut};
use std::collections::{HashSet, VecDeque};

/// A single row of the map's tiles
pub type Row = [Option<Tile>];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomId(usize);

impl fmt::Display for RoomId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A type that represents the static floor plan of a map
#[derive(Clone)]
pub struct FloorMap {
    tiles: Vec<Vec<Option<Tile>>>,
    /// The RoomId is the index into this field
    rooms: Vec<Room>,
    /// The sprite used to render empty tiles (i.e. when there is no tile)
    empty_tile_sprite: SpriteImage,
}

impl Index<usize> for FloorMap {
    type Output = Row;

    fn index(&self, index: usize) -> &Self::Output {
        self.tiles.index(index)
    }
}

impl IndexMut<usize> for FloorMap {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.tiles.index_mut(index)
    }
}

impl fmt::Debug for FloorMap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use colored::*;

        for row in self.rows() {
            for tile in row {
                match tile {
                    None => write!(f, "{}", " ".on_black())?,
                    Some(tile) => match tile.ttype {
                        TileType::Passageway => {
                            write!(f, "{}", tile.walls.to_string().on_green())?
                        },
                        TileType::Room(id) => {
                            let object = tile.object.as_ref().map(|o| o.to_string().bold())
                                .unwrap_or_else(|| tile.walls.to_string().black());
                            write!(f, "{}", match self.room(id).room_type() {
                                RoomType::Normal => object.on_blue(),
                                RoomType::Challenge => object.on_red(),
                                RoomType::PlayerStart => object.on_bright_blue(),
                                RoomType::TreasureChamber => object.on_yellow(),
                            })?
                        },
                    },
                }
            }
            writeln!(f)?;
        }

        Ok(())
    }
}

impl FloorMap {
    /// Create a new FloorMap with the given number of rows and columns
    pub fn new(rows: usize, cols: usize, empty_tile_sprite: SpriteImage) -> Self {
        assert!(rows > 0 && cols > 0, "Cannot create a grid with zero rows or columns");
        FloorMap {
            tiles: vec![vec![None; cols]; rows],
            rooms: Vec::new(),
            empty_tile_sprite,
        }
    }

    /// Returns the sprite that should be used to render empty tiles (i.e. when there is no tile)
    pub fn empty_tile_sprite(&self) -> SpriteImage {
        self.empty_tile_sprite
    }

    /// Returns the number of rows in this grid
    pub fn rows_len(&self) -> usize {
        self.tiles.len()
    }

    /// Returns the number of columns in this grid
    pub fn cols_len(&self) -> usize {
        self[0].len()
    }

    /// Returns an iterator over each row
    pub fn rows(&self) -> impl Iterator<Item=&Row> {
        self.tiles.iter().map(|r| r.as_slice())
    }

    /// Gets the tile at the given position (or None if empty)
    pub fn get(&self, TilePos {row, col}: TilePos) -> Option<&Tile> {
        self[row][col].as_ref()
    }

    /// Gets the tile at the given position (or None if empty)
    pub fn get_mut(&mut self, TilePos {row, col}: TilePos) -> Option<&mut Tile> {
        self[row][col].as_mut()
    }

    /// Returns true if the given position is empty (no tile)
    pub fn is_empty(&self, TilePos {row, col}: TilePos) -> bool {
        self[row][col].is_none()
    }

    /// Returns true if the given position is part of the room with the given ID
    pub fn is_room_id(&self, TilePos {row, col}: TilePos, room_id: RoomId) -> bool {
        match self[row][col] {
            Some(Tile {ttype: TileType::Room(id), ..}) => id == room_id,
            _ => false,
        }
    }

    /// Returns true if the given position is a dead end passageway
    pub fn is_dead_end(&self, TilePos {row, col}: TilePos) -> bool {
        match self[row][col] {
            Some(Tile {ttype: TileType::Passageway, ref walls, ..}) => walls.is_dead_end(),
            _ => false,
        }
    }

    /// Returns true if the given position is passageway
    pub fn is_passageway(&self, TilePos {row, col}: TilePos) -> bool {
        match self[row][col] {
            Some(Tile {ttype: TileType::Passageway, ..}) => true,
            _ => false,
        }
    }

    pub fn rooms(&self) -> impl Iterator<Item=&Room> {
        self.rooms.iter()
    }

    pub fn room(&self, room_id: RoomId) -> &Room {
        &self.rooms[room_id.0]
    }

    /// Add a room to the map. Rooms should NOT be overlapping, though this condition is NOT
    /// checked by this method. Hence why this is private.
    pub(in super) fn add_room(&mut self, room: Room) -> RoomId {
        self.rooms.push(room);
        RoomId(self.rooms.len() - 1)
    }

    /// Places a tile with the given type at the given location
    ///
    /// Panics if that location was not previously empty
    pub fn place_tile(&mut self, TilePos {row, col}: TilePos, ttype: TileType, sprite: SpriteImage) {
        let tile = &mut self[row][col];
        // Should not be any other tile here already
        debug_assert!(tile.is_none(),
            "bug: attempt to place tile on a position where a tile was already placed");
        *tile = Some(Tile::with_type(ttype, sprite));
    }

    /// Removes a passageway from the map and closes any walls around it
    pub(in super) fn remove_passageway(&mut self, pos: TilePos) {
        assert!(self.is_passageway(pos), "bug: remove passageway can only be called on a passageway tile");
        let adjacents: Vec<_> = self.adjacent_positions(pos)
            .filter(|&pt| !self.is_empty(pt))
            .collect();

        for adj in adjacents {
            self.close_between(pos, adj);
        }

        self[pos.row][pos.col] = None;
    }

    /// Returns true if there is NO wall between two adjacent cells
    pub fn is_open_between(&self, pos1: TilePos, pos2: TilePos) -> bool {
        macro_rules! wall_is_open {
            ($dir:ident, $opp:ident) => {
                match (self.get(pos1), self.get(pos2)) {
                    (Some(tile1), Some(tile2)) => {
                        debug_assert_eq!(tile1.walls.$dir, tile2.walls.$opp);
                        tile1.walls.$dir == Wall::Open
                    },
                    // If either option is an empty tile then by definition we cannot have an open
                    // wall since that would lead nowhere!
                    (Some(tile1), None) => {
                        debug_assert_eq!(tile1.walls.$dir, Wall::Closed);
                        false
                    },
                    (None, Some(tile2)) => {
                        debug_assert_eq!(tile2.walls.$opp, Wall::Closed);
                        false
                    },
                    (None, None) => false,
                }
            };
        }
        match pos2.difference(pos1) {
            // second position is north of first position
            (-1, 0) => wall_is_open!(north, south),
            // second position is east of first position
            (0, 1) => wall_is_open!(east, west),
            // second position is south of first position
            (1, 0) => wall_is_open!(south, north),
            // second position is west of first position
            (0, -1) => wall_is_open!(west, east),
            _ => unreachable!("bug: attempt to check if two non-adjacent cells have an open wall between them"),
        }
    }

    /// Removes the wall between two adjacent cells
    pub fn open_between(&mut self, pos1: TilePos, pos2: TilePos) {
        macro_rules! open {
            ($wall1:ident, $wall2:ident) => {
                {
                    self.get_mut(pos1).expect("Cannot open a wall to an empty tile").walls.$wall1 = Wall::Open;
                    self.get_mut(pos2).expect("Cannot open a wall to an empty tile").walls.$wall2 = Wall::Open;
                }
            };
        }
        match pos2.difference(pos1) {
            // second position is north of first position
            (-1, 0) => open!(north, south),
            // second position is east of first position
            (0, 1) => open!(east, west),
            // second position is south of first position
            (1, 0) => open!(south, north),
            // second position is west of first position
            (0, -1) => open!(west, east),
            _ => unreachable!("bug: attempt to open a wall between two non-adjacent cells"),
        }
    }

    /// Adds a wall between two adjacent cells
    pub fn close_between(&mut self, pos1: TilePos, pos2: TilePos) {
        macro_rules! close {
            ($wall1:ident, $wall2:ident) => {
                {
                    // Note that walls aren't allowed to be open when there is no tile on the other side
                    self.get_mut(pos1).expect("Cannot close a wall to an empty tile").walls.$wall1 = Wall::Closed;
                    self.get_mut(pos2).expect("Cannot close a wall to an empty tile").walls.$wall2 = Wall::Closed;
                }
            };
        }
        match pos2.difference(pos1) {
            // second position is north of first position
            (-1, 0) => close!(north, south),
            // second position is east of first position
            (0, 1) => close!(east, west),
            // second position is south of first position
            (1, 0) => close!(south, north),
            // second position is west of first position
            (0, -1) => close!(west, east),
            _ => unreachable!("bug: attempt to open a wall between two non-adjacent cells"),
        }
    }

    /// Returns an iterator over the positions of all tiles contained within this map
    pub fn tile_positions(&self) -> impl Iterator<Item=TilePos> {
        let cols = self.cols_len();
        (0..self.rows_len()).flat_map(move |row| (0..cols).map(move |col| TilePos {row, col}))
    }

    /// Returns an iterator of tile positions adjacent to the given tile in the four cardinal
    /// directions. Only returns valid cell positions.
    pub fn adjacent_positions(&self, TilePos {row, col}: TilePos) -> impl Iterator<Item=TilePos> + '_ {
        [(-1, 0), (0, -1), (1, 0), (0, 1)].into_iter().filter_map(move |(row_offset, col_offset)| {
            let row = row as isize + row_offset;
            let col = col as isize + col_offset;

            if row < 0 || row >= self.rows_len() as isize || col < 0 || col >= self.cols_len() as isize {
                None
            } else {
                Some(TilePos {row: row as usize, col: col as usize})
            }
        })
    }

    /// Returns an iterator of adjacent passages that do not have a wall between them and the
    /// given position.
    pub fn adjacent_open_passages(&self, pos: TilePos) -> impl Iterator<Item=TilePos> + '_ {
        self.adjacent_positions(pos)
            .filter(move |&pt| self.is_passageway(pt) && self.is_open_between(pos, pt))
    }

    /// Executes a depth-first search starting from a given tile
    ///
    /// Takes a closure that is given the next position "node" to be processed and its
    /// adjacents. The closure should return the adjacents that you want it to keep searching.
    ///
    /// Returns the positions that were visited
    pub fn depth_first_search_mut<F>(&mut self, start: TilePos, mut next_adjacents: F) -> HashSet<TilePos>
        where F: FnMut(&mut Self, TilePos, Vec<TilePos>) -> Vec<TilePos> {

        let mut seen = HashSet::new();
        let mut open = VecDeque::new();
        open.push_front(start);

        while let Some(node) = open.pop_front() {
            if seen.contains(&node) {
                continue;
            }
            seen.insert(node);

            let adjacents = self.adjacent_positions(node).filter(|pt| !seen.contains(pt)).collect();
            let mut adjacents = next_adjacents(self, node, adjacents).into_iter();

            // This is a depth first search, so we insert the first element and append the rest
            if let Some(adj) = adjacents.next() {
                open.push_front(adj);
            }
            for adj in adjacents {
                open.push_back(adj);
            }
        }

        seen
    }
}
