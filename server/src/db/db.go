package db

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"os"
	"time"

	"github.com/sevenautumns/niketsu/server/src/config"
	"go.etcd.io/bbolt"
)

const (
	PlaylistKey = "playlist"
	VideoKey    = "video"
	PositionKey = "position"
)

// Functionality for updating the states. Buckets and keys are only created
// based on the Update() function.
type KeyValueStore interface {
	Open() error
	Close() error
	Delete() error
	Update(bucket string, key string, value []byte) error
	GetValue(bucket string, key string) ([]byte, error)
	DeleteKey(bucket string, key string) error
	DeleteBucket(bucket string) error
	UpdatePlaylist(bucket string, playlist []byte, video string, position uint64) error
	GetRoomConfigs(bucket string) (map[string]config.RoomConfig, error)
}

type BoltKeyValueStore struct {
	db      *bbolt.DB
	path    string
	timeout time.Duration
}

type DBManager struct {
	db KeyValueStore
}

func NewBoltKeyValueStore(path string, timeout uint64) (*BoltKeyValueStore, error) {
	if path == "" || timeout == 0 {
		return nil, errors.New("invalid parameters. path should not be empty and timeout should be non-zero.")
	}

	return &BoltKeyValueStore{path: path, timeout: time.Duration(timeout * uint64(time.Second))}, nil
}

func (keyValueStore *BoltKeyValueStore) Open() error {
	conn, err := bbolt.Open(keyValueStore.path, 0600, &bbolt.Options{Timeout: keyValueStore.timeout})
	if err != nil {
		return err
	}
	keyValueStore.db = conn

	return nil
}

func (keyValueStore BoltKeyValueStore) Close() error {
	if keyValueStore.db == nil {
		return errors.New("Database not initialized. Can not call Close()")
	}
	return keyValueStore.db.Close()
}

func (keyValueStore BoltKeyValueStore) Delete() error {
	return os.Remove(keyValueStore.path)
}

func (keyValueStore BoltKeyValueStore) Update(bucket string, key string, value []byte) error {
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
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

	return err
}

func (keyValueStore BoltKeyValueStore) GetValue(bucket string, key string) ([]byte, error) {
	var val []byte
	err := keyValueStore.db.View(func(tx *bbolt.Tx) error {
		b := tx.Bucket([]byte(bucket))

		if b == nil {
			return bbolt.ErrBucketNotFound
		}
		val = b.Get([]byte(key))

		return nil
	})

	if err != nil {
		return nil, err
	}

	return val, nil
}

func (keyValueStore BoltKeyValueStore) DeleteKey(bucket string, key string) error {
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		b := tx.Bucket([]byte(bucket))
		err := b.Delete([]byte(key))

		return err
	})

	return err
}

func (keyValueStore BoltKeyValueStore) DeleteBucket(bucket string) error {
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		err := tx.DeleteBucket([]byte(bucket))

		return err
	})

	return err
}

func (keyValueStore BoltKeyValueStore) UpdatePlaylist(bucket string, playlist []byte, video string, position uint64) error {
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		err = b.Put([]byte(VideoKey), []byte(video))
		if err != nil {
			return err
		}

		pos := make([]byte, 8)
		binary.LittleEndian.PutUint64(pos, position)
		err = b.Put([]byte(PositionKey), pos)
		if err != nil {
			return err
		}

		err = b.Put([]byte(PlaylistKey), playlist)
		if err != nil {
			return err
		}

		return nil
	})

	return err
}

func (keyValueStore BoltKeyValueStore) GetRoomConfigs(bucket string) (map[string]config.RoomConfig, error) {
	roomConfigs := make(map[string]config.RoomConfig, 0)
	err := keyValueStore.db.Update(func(tx *bbolt.Tx) error {
		b, err := tx.CreateBucketIfNotExists([]byte(bucket))
		if err != nil {
			return err
		}

		b.ForEach(func(k, v []byte) error {
			var roomConfig config.RoomConfig
			err := json.Unmarshal(v, &roomConfig)
			if err != nil {
				return err
			}
			roomConfigs[string(k[:])] = roomConfig

			return nil
		})

		return nil
	})

	if err != nil {
		return nil, err
	}

	return roomConfigs, nil
}

func NewDBManager(keyValueStore KeyValueStore) DBManager {
	return DBManager{db: keyValueStore}
}

func (dbManager DBManager) Open() error {
	return dbManager.db.Open()
}

func (dbManager DBManager) Close() error {
	return dbManager.db.Close()
}

func (dbManager DBManager) Delete() error {
	return dbManager.db.Delete()
}

func (dbManager DBManager) Update(bucket string, key string, value []byte) error {
	return dbManager.db.Update(bucket, key, value)
}

func (dbManager DBManager) GetValue(bucket string, key string) ([]byte, error) {
	return dbManager.db.GetValue(bucket, key)
}

func (dbManager DBManager) DeleteKey(bucket string, key string) error {
	return dbManager.db.DeleteKey(bucket, key)
}

func (dbManager DBManager) DeleteBucket(bucket string) error {
	return dbManager.db.DeleteBucket(bucket)
}

func (dbManager DBManager) UpdatePlaylist(bucket string, playlist []byte, video string, position uint64) error {
	return dbManager.db.UpdatePlaylist(bucket, playlist, video, position)
}

func (dbManager DBManager) GetRoomConfigs(bucket string) (map[string]config.RoomConfig, error) {
	return dbManager.db.GetRoomConfigs(bucket)
}
