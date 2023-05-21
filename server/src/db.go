package niketsu_server

import (
	"encoding/binary"
	"encoding/json"
	"time"

	bolt "go.etcd.io/bbolt"
)

type DB struct {
	path                string
	conn                *bolt.DB
	timeout             time.Duration
	statisticsFrequency time.Duration
	lastStatistics      bolt.Stats
}

// Creates a new Database connection with some additional options.
// Make sure to call defer karen.Close() after successfully initializing the database.
// timeout and statisticsFrequency are given in seconds.
func NewDB(path string, timeout uint64, statFreq uint64) (DB, error) {
	var db DB
	db.path = path
	db.timeout = time.Duration(timeout * uint64(time.Second))
	db.statisticsFrequency = time.Duration(statFreq * uint64(time.Second))

	conn, err := bolt.Open(db.path, 0600, &bolt.Options{Timeout: db.timeout})
	if err != nil {
		logger.Warnw("Failed to open database", "error", err)
	}
	db.conn = conn
	db.lastStatistics = db.conn.Stats()

	return db, nil
}

// Closes the database connection if it is still not nil
func (db *DB) Close() {
	db.conn.Close()
}

// Monitors statistics of the database in given intervals of statFreq.
func (db *DB) Monitor() {
	for {
		time.Sleep(db.statisticsFrequency)
		db.printStatistics()
		db.updateStatistics()
	}
}

func (db *DB) printStatistics() {
	stats := db.conn.Stats()
	diff := stats.Sub(&db.lastStatistics)

	encoded, err := json.Marshal(diff)
	if err != nil {
		logger.Warnw("An error occured creating stats diff", "err", err)
	} else {
		logger.Infow("Current stats", "stats", string(encoded))
	}
}

func (db *DB) updateStatistics() {
	db.lastStatistics = db.conn.Stats()
}

func (db *DB) UpdatePlaylist(bucket string, playlist []byte, video string, position uint64) {
	err := db.conn.Update(func(tx *bolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		err = b.Put([]byte("video"), []byte(video))
		if err != nil {
			return err
		}

		pos := make([]byte, 8)
		binary.LittleEndian.PutUint64(pos, position)
		err = b.Put([]byte("position"), pos)
		if err != nil {
			return err
		}

		err = b.Put([]byte("playlist"), playlist)
		if err != nil {
			return err
		}

		return nil
	})

	if err != nil {
		logger.Warnw("Update playlist transaction failed", "db", db.path, "error", err)
	}
}

func (db *DB) Update(bucket string, key string, value []byte) {
	err := db.conn.Update(func(tx *bolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		err = b.Put([]byte(key), value)
		if err != nil {
			return err
		}

		return nil
	})

	if err != nil {
		logger.Warnw("Update key/value transaction failed", "db", db.path, "error", err)
	}
}

func (db *DB) Get(bucket string, key string) ([]byte, error) {
	var val []byte
	err := db.conn.View(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucket))

		if b == nil {
			return bolt.ErrBucketNotFound
		}
		val = b.Get([]byte(key))

		return nil
	})

	if err != nil {
		return nil, err
	}

	return val, nil
}

func (db *DB) DeleteBucket(bucket string) {
	err := db.conn.Update(func(tx *bolt.Tx) error {
		err := tx.DeleteBucket([]byte(bucket))

		return err
	})

	if err != nil {
		logger.Warnw("Delete bucket transaction failed", "db", db.path, "error", err)
	}
}

func (db *DB) DeleteKey(bucket string, key string) {
	err := db.conn.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucket))
		err := b.Delete([]byte(key))

		return err
	})

	if err != nil {
		logger.Warnw("Delete key transaction failed", "db", db.path, "error", err)
	}
}

func (db *DB) GetRoomConfigs(bucket string) map[string]RoomConfig {
	roomConfigs := make(map[string]RoomConfig, 0)
	err := db.conn.Update(func(tx *bolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		b.ForEach(func(k, v []byte) error {
			var rc RoomConfig
			err := json.Unmarshal(v, &rc)
			if err != nil {
				return err
			}
			roomConfigs[string(k[:])] = rc

			return nil
		})

		return nil
	})

	if err != nil {
		return nil
	}

	return roomConfigs
}
