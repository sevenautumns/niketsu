// Code generated by MockGen. DO NOT EDIT.
// Source: db.go

// Package db is a generated GoMock package.
package db

import (
	reflect "reflect"

	gomock "github.com/golang/mock/gomock"
)

// MockDBManager is a mock of DBManager interface.
type MockDBManager struct {
	ctrl     *gomock.Controller
	recorder *MockDBManagerMockRecorder
}

// MockDBManagerMockRecorder is the mock recorder for MockDBManager.
type MockDBManagerMockRecorder struct {
	mock *MockDBManager
}

// NewMockDBManager creates a new mock instance.
func NewMockDBManager(ctrl *gomock.Controller) *MockDBManager {
	mock := &MockDBManager{ctrl: ctrl}
	mock.recorder = &MockDBManagerMockRecorder{mock}
	return mock
}

// EXPECT returns an object that allows the caller to indicate expected use.
func (m *MockDBManager) EXPECT() *MockDBManagerMockRecorder {
	return m.recorder
}

// Close mocks base method.
func (m *MockDBManager) Close() error {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "Close")
	ret0, _ := ret[0].(error)
	return ret0
}

// Close indicates an expected call of Close.
func (mr *MockDBManagerMockRecorder) Close() *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "Close", reflect.TypeOf((*MockDBManager)(nil).Close))
}

// Delete mocks base method.
func (m *MockDBManager) Delete() error {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "Delete")
	ret0, _ := ret[0].(error)
	return ret0
}

// Delete indicates an expected call of Delete.
func (mr *MockDBManagerMockRecorder) Delete() *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "Delete", reflect.TypeOf((*MockDBManager)(nil).Delete))
}

// DeleteBucket mocks base method.
func (m *MockDBManager) DeleteBucket(bucket string) error {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "DeleteBucket", bucket)
	ret0, _ := ret[0].(error)
	return ret0
}

// DeleteBucket indicates an expected call of DeleteBucket.
func (mr *MockDBManagerMockRecorder) DeleteBucket(bucket interface{}) *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "DeleteBucket", reflect.TypeOf((*MockDBManager)(nil).DeleteBucket), bucket)
}

// DeleteKey mocks base method.
func (m *MockDBManager) DeleteKey(bucket, key string) error {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "DeleteKey", bucket, key)
	ret0, _ := ret[0].(error)
	return ret0
}

// DeleteKey indicates an expected call of DeleteKey.
func (mr *MockDBManagerMockRecorder) DeleteKey(bucket, key interface{}) *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "DeleteKey", reflect.TypeOf((*MockDBManager)(nil).DeleteKey), bucket, key)
}

// GetAll mocks base method.
func (m *MockDBManager) GetAll(bucket string) (map[string][]byte, error) {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "GetAll", bucket)
	ret0, _ := ret[0].(map[string][]byte)
	ret1, _ := ret[1].(error)
	return ret0, ret1
}

// GetAll indicates an expected call of GetAll.
func (mr *MockDBManagerMockRecorder) GetAll(bucket interface{}) *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "GetAll", reflect.TypeOf((*MockDBManager)(nil).GetAll), bucket)
}

// GetValue mocks base method.
func (m *MockDBManager) GetValue(bucket, key string) ([]byte, error) {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "GetValue", bucket, key)
	ret0, _ := ret[0].([]byte)
	ret1, _ := ret[1].(error)
	return ret0, ret1
}

// GetValue indicates an expected call of GetValue.
func (mr *MockDBManagerMockRecorder) GetValue(bucket, key interface{}) *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "GetValue", reflect.TypeOf((*MockDBManager)(nil).GetValue), bucket, key)
}

// Open mocks base method.
func (m *MockDBManager) Open() error {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "Open")
	ret0, _ := ret[0].(error)
	return ret0
}

// Open indicates an expected call of Open.
func (mr *MockDBManagerMockRecorder) Open() *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "Open", reflect.TypeOf((*MockDBManager)(nil).Open))
}

// Update mocks base method.
func (m *MockDBManager) Update(bucket, key string, value []byte) error {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "Update", bucket, key, value)
	ret0, _ := ret[0].(error)
	return ret0
}

// Update indicates an expected call of Update.
func (mr *MockDBManagerMockRecorder) Update(bucket, key, value interface{}) *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "Update", reflect.TypeOf((*MockDBManager)(nil).Update), bucket, key, value)
}

// UpdatePlaylist mocks base method.
func (m *MockDBManager) UpdatePlaylist(bucket string, playlist []byte, video string, position uint64) error {
	m.ctrl.T.Helper()
	ret := m.ctrl.Call(m, "UpdatePlaylist", bucket, playlist, video, position)
	ret0, _ := ret[0].(error)
	return ret0
}

// UpdatePlaylist indicates an expected call of UpdatePlaylist.
func (mr *MockDBManagerMockRecorder) UpdatePlaylist(bucket, playlist, video, position interface{}) *gomock.Call {
	mr.mock.ctrl.T.Helper()
	return mr.mock.ctrl.RecordCallWithMethodType(mr.mock, "UpdatePlaylist", reflect.TypeOf((*MockDBManager)(nil).UpdatePlaylist), bucket, playlist, video, position)
}
