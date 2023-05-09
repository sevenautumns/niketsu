package niketsu_server

import (
	"encoding/binary"
	"encoding/json"
	"time"

	bolt "go.etcd.io/bbolt"
)

type Karen struct {
	path     string
	timeout  time.Duration
	statFreq time.Duration
	db       *bolt.DB
}

// Creates a new Database connection with some additional options. Make sure to call defer karen.Close() after successfully initializing the database. timeout and statFreq are given in seconds.
// If opendb is given, the connection to the db is directly opened. Otherwise, after each function call, the connection is opened and closed again.
func NewKaren(path string, timeout uint64, statFreq uint64) (Karen, error) {
	var karen Karen
	karen.path = path
	karen.timeout = time.Duration(timeout * uint64(time.Second))
	karen.statFreq = time.Duration(statFreq * uint64(time.Second))

	db, err := bolt.Open(karen.path, 0600, &bolt.Options{Timeout: karen.timeout})
	if err != nil {
		logger.Warnw("Failed to open database", "error", err)
	}
	karen.db = db

	return karen, nil
}

// Closes the database connection if it is still not nil
func (karen *Karen) Close() {
	karen.db.Close()
}

// Monitors statistics of the database in given intervals of statFreq.
func (karen *Karen) Monitor() {
	prev := karen.db.Stats()
	for {
		time.Sleep(karen.statFreq)

		stats := karen.db.Stats()
		diff := stats.Sub(&prev)
		encoded, err := json.Marshal(diff)
		if err != nil {
			logger.Warnw("Error occured creating stats diff", "err", err)
		} else {
			logger.Infow("Current stats", "stats", string(encoded))
		}

		prev = stats
	}
}

// Updates a given bucket with a playlist, video and position.
func (karen *Karen) UpdatePlaylist(bucket string, playlist []byte, video string, position uint64) {
	err := karen.db.Update(func(tx *bolt.Tx) error {
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
		logger.Warnw("Update playlist transaction failed", "db", karen.path, "error", err)
	}
}

// Updates a given bucket with a key and a value.
func (karen *Karen) Update(bucket string, key string, value []byte) {
	err := karen.db.Update(func(tx *bolt.Tx) error {
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
		logger.Warnw("Update key/value transaction failed", "db", karen.path, "error", err)
	}
}

// Returns value of a key from a specified bucket.
func (karen *Karen) Get(bucket string, key string) ([]byte, error) {
	var val []byte
	err := karen.db.View(func(tx *bolt.Tx) error {
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

// Tries to delete a bucket.
func (karen *Karen) DeleteBucket(bucket string) {
	err := karen.db.Update(func(tx *bolt.Tx) error {
		err := tx.DeleteBucket([]byte(bucket))

		return err
	})

	if err != nil {
		logger.Warnw("Delete bucket transaction failed", "db", karen.path, "error", err)
	}
}

// Tries to delete a key from a bucket.
func (karen *Karen) DeleteKey(bucket string, key string) {
	err := karen.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket([]byte(bucket))
		err := b.Delete([]byte(key))

		return err
	})

	if err != nil {
		logger.Warnw("Delete key transaction failed", "db", karen.path, "error", err)
	}
}

// Tries to get all keys (room) from a bucket (top-level).
func (karen *Karen) GetRoomConfigs(bucket string) map[string]RoomConfig {
	roomConfigs := make(map[string]RoomConfig, 0)
	err := karen.db.Update(func(tx *bolt.Tx) error {
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
